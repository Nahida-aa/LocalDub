//! Generic filesystem watching, modeled on Zed's `crates/fs/src/fs_watcher.rs`
//! but stripped of editor-only complexity (case-insensitive name matching,
//! WSL/network-filesystem detection, gpui executor).
//!
//! Design:
//! - A process-global [`GlobalWatcher`] owns one native (`notify::RecommendedWatcher`)
//!   and one poll (`notify::PollWatcher`) backend, plus a dedicated dispatch thread.
//! - All watches are recursive, so directories created *after* the watch starts are
//!   still observed (a pipeline's `mix_video/` appearing mid-run and the files
//!   inside it). `watch(path)` returns an [`FsWatch`] handle whose `Drop` unwatches.
//! - `watch_stream(path)` yields a `Stream<PathEvent>`. Events are coalesced by path
//!   (like Zed's `pending_path_events`): many changes to the same file collapse into
//!   one final event, and bursts are flushed in batches. On top of path-coalescing we
//!   also apply a short **flush debounce window**: after the first wake-up signal we
//!   wait `LOCALDUB_FILE_WATCHER_FLUSH_MS` (default 250ms) before draining, so a file
//!   written in chunks across multiple poll cycles (e.g. ffmpeg streaming an .mp4)
//!   collapses into a single `Changed` event rather than one-per-chunk. `Access`
//!   events are dropped (they carry no content change).

use crate::event::{PathEvent, PathEventKind, WatcherMode};
use async_channel::{Receiver, Sender};
use futures::stream::{self, Stream};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

/// Control which backend is used, via `LOCALDUB_FILE_WATCHER_MODE`.
/// `auto` (default) currently resolves to Native; kept as a hook for future
/// filesystem-type detection.
fn requires_poll_watcher() -> bool {
    match std::env::var("LOCALDUB_FILE_WATCHER_MODE")
        .as_deref()
        .unwrap_or("auto")
    {
        "native" => false,
        "poll" => true,
        _ => false,
    }
}

fn poll_interval() -> Duration {
    static POLL_INTERVAL: OnceLock<Duration> = OnceLock::new();
    *POLL_INTERVAL.get_or_init(|| {
        let ms: u64 = std::env::var("LOCALDUB_FILE_WATCHER_POLL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2000)
            .clamp(500, 30_000);
        Duration::from_millis(ms)
    })
}

/// How long to wait after the first wake-up signal before draining the pending
/// buffer. Mirrors the time-window coalescing that Zed applies on top of its
/// path-based `pending_path_events`.
///
/// Why this is needed: path-coalescing alone only collapses events that land in
/// the *same* drain cycle. A file written in chunks spread across multiple poll
/// intervals (e.g. ffmpeg streaming an .mp4) produces one wake-up per chunk, so
/// each chunk drains as its own `Changed` event. Waiting a short window lets the
/// burst of chunks accumulate into a single coalesced event.
///
/// Configured via `LOCALDUB_FILE_WATCHER_FLUSH_MS`, clamped to 50–2000ms,
/// default 250ms.
fn flush_debounce_ms() -> Duration {
    static FLUSH_DEBOUNCE: OnceLock<Duration> = OnceLock::new();
    *FLUSH_DEBOUNCE.get_or_init(|| {
        let ms: u64 = std::env::var("LOCALDUB_FILE_WATCHER_FLUSH_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(250)
            .clamp(50, 2_000);
        Duration::from_millis(ms)
    })
}

/// Handle returned by [`watch`]. Dropping it removes the underlying watch.
#[derive(Debug)]
pub struct FsWatch {
    id: WatcherRegistrationId,
}

impl Drop for FsWatch {
    fn drop(&mut self) {
        global_watcher().remove(self.id);
    }
}

/// Register `path` to be watched. `path` should be a directory; callers that
/// care about a single file should watch its parent directory and filter by
/// name in their event handling (this survives files being truncated/recreated).
///
/// Returns an [`FsWatch`] handle; the watch is active until the handle is dropped.
pub fn watch(path: impl Into<PathBuf>) -> io::Result<FsWatch> {
    let id = global_watcher().add(path.into())?;
    Ok(FsWatch { id })
}

/// Watch `path` and yield a stream of [`PathEvent`]s covering it. The underlying
/// watch is tied to the returned stream's lifetime: dropping the stream unwatches.
pub fn watch_stream(path: impl Into<PathBuf>) -> io::Result<impl Stream<Item = PathEvent>> {
    let handle = watch(path.into())?;
    // Per-stream pending buffer. Like Zed's `pending_path_events`, events are
    // coalesced by path: if the same path changes many times before the stream
    // is polled, only the latest kind survives (e.g. a file written in 30 chunks
    // yields a single `Changed`). The `signal` channel wakes the stream to flush
    // the buffer in batches.
    let pending: Arc<Mutex<HashMap<PathBuf, Option<PathEventKind>>>> = Arc::default();
    let (signal_tx, signal_rx): (Sender<()>, Receiver<()>) = async_channel::unbounded();

    global_watcher().install_callback(handle.id, {
        let pending = pending.clone();
        move |event: &Event| {
            let mut map = pending.lock().unwrap();
            let mut dirty = false;
            for path_event in map_notify_event(event) {
                map.insert(path_event.path.clone(), path_event.kind);
                dirty = true;
            }
            if dirty {
                let _ = signal_tx.try_send(());
            }
        }
    });

    // State: signal receiver, coalescing buffer, watch handle, and a queue of
    // already-batched items awaiting yield.
    let flush_debounce = flush_debounce_ms();
    let state = (
        signal_rx,
        pending,
        handle,
        Vec::<PathEvent>::new(),
        flush_debounce,
    );
    Ok(stream::unfold(
        state,
        |(signal_rx, pending, handle, mut queue, flush_debounce)| async move {
            loop {
                // Yield anything already batched first.
                if let Some(item) = queue.pop() {
                    return Some((item, (signal_rx, pending, handle, queue, flush_debounce)));
                }
                // Wait for a wake-up. The first signal opens a short flush window:
                // any further signals that arrive within that window are drained
                // out quickly (non-blocking `try_recv`) so their events accumulate
                // in `pending` instead of each triggering its own flush. Only after
                // the window elapses do we drain the whole buffer in one batch.
                if signal_rx.recv().await.is_err() {
                    return None;
                }
                tokio::time::sleep(flush_debounce).await;
                // Consume any signals that piled up during the window without
                // blocking forever; bounded by the window size so this can't loop
                // indefinitely.
                let deadline = tokio::time::Instant::now() + flush_debounce;
                while tokio::time::Instant::now() < deadline {
                    match signal_rx.try_recv() {
                        Ok(()) => continue,
                        Err(_) => break,
                    }
                }
                let batch: Vec<PathEvent> = {
                    let mut map = pending.lock().unwrap();
                    std::mem::take(&mut *map)
                        .into_iter()
                        .map(|(path, kind)| PathEvent::new(path, kind))
                        .collect()
                };
                // If the wake-up was spurious (pending already drained by a prior
                // cycle) or nothing accumulated, loop back to wait for the next
                // signal instead of yielding a dummy event.
                if batch.is_empty() {
                    continue;
                }
                queue = batch;
            }
        },
    ))
}

fn map_notify_event(event: &Event) -> Vec<PathEvent> {
    // Access events (open/close/read/stat) carry no content change and fire
    // extremely often; drop them like Zed does (see `enqueue`). This is what
    // produced the `kind: null` entries on the frontend.
    if matches!(event.kind, EventKind::Access(_)) {
        return Vec::new();
    }

    // Debug: print the raw notify EventKind so we can see what falls through to
    // `None` (the `kind: null` entries on the frontend). Enable with RUST_LOG=fs=debug.
    tracing::debug!(raw_kind = ?event.kind, paths = ?event.paths, "fs watcher raw notify event");
    let kind = match event.kind {
        EventKind::Create(_) => Some(PathEventKind::Created),
        EventKind::Modify(_) => Some(PathEventKind::Changed),
        EventKind::Remove(_) => Some(PathEventKind::Removed),
        _ => None,
    };
    let mut events: Vec<PathEvent> = event
        .paths
        .iter()
        .map(|p| PathEvent::new(p.clone(), kind))
        .collect();
    if event.need_rescan() {
        events.retain(|e| e.kind != Some(PathEventKind::Rescan));
        let p = event.paths.first().cloned().unwrap_or_default();
        events.push(PathEvent::new(p, Some(PathEventKind::Rescan)));
    }
    tracing::debug!(mapped = ?events, "fs watcher mapped notify event");
    events
}

// ----------------------------------------------------------------------------
// GlobalWatcher internals
// ----------------------------------------------------------------------------

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct WatcherRegistrationId(u32);

struct RegistrationState {
    callback: Arc<dyn Fn(&Event) + Send + Sync>,
    path: Arc<Path>,
    mode: WatcherMode,
}

struct GlobalWatcher {
    state: Mutex<WatcherState>,
    // Separate lock from `state`: notify callbacks fire while we hold the
    // watcher lock, and those callbacks read `state`, so the two must not be
    // nested in the wrong order.
    native_watcher: Mutex<Option<Box<dyn WatchBackend>>>,
    poll_watcher: Mutex<Option<Box<dyn WatchBackend>>>,
    event_tx: Sender<(WatcherMode, notify::Result<Event>)>,
}

struct WatcherState {
    registrations: HashMap<WatcherRegistrationId, RegistrationState>,
    last_id: WatcherRegistrationId,
    // path -> set of registration ids watching it (or an ancestor of it)
    path_index: HashMap<Arc<Path>, Vec<WatcherRegistrationId>>,
}

impl WatcherState {
    fn next_id(&mut self) -> WatcherRegistrationId {
        let id = self.last_id;
        self.last_id = WatcherRegistrationId(id.0.wrapping_add(1));
        id
    }
}

/// Object-safe wrapper around a `notify::Watcher` backend. We box the two
/// concrete backends so `add`/`remove` share one call site.
trait WatchBackend: Send {
    fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()>;
    fn unwatch(&mut self, path: &Path) -> notify::Result<()>;
}

impl WatchBackend for notify::RecommendedWatcher {
    fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        notify::Watcher::watch(self, path, mode)
    }
    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        notify::Watcher::unwatch(self, path)
    }
}

impl WatchBackend for notify::PollWatcher {
    fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        notify::Watcher::watch(self, path, mode)
    }
    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        notify::Watcher::unwatch(self, path)
    }
}

impl GlobalWatcher {
    fn add(&self, path: PathBuf) -> io::Result<WatcherRegistrationId> {
        let mode = if requires_poll_watcher() {
            WatcherMode::Poll
        } else {
            WatcherMode::Native
        };
        let arc_path: Arc<Path> = Arc::from(path.as_path());

        // Touch the OS watcher outside the state lock.
        self.ensure_watcher(mode)
            .map_err(|e| io::Error::other(e.to_string()))?;
        {
            let mut w = match mode {
                WatcherMode::Native => self.native_watcher.lock().unwrap(),
                WatcherMode::Poll => self.poll_watcher.lock().unwrap(),
            };
            // Use Recursive on every platform and backend. notify's `NonRecursive`
            // (the old Linux default) does NOT auto-track subdirectories created
            // after the watch is installed, so dynamically-created dirs (e.g. a
            // pipeline's `mix_video/` appearing mid-run) and their files would
            // never surface events. Recursive keeps those in view. Poll already
            // used Recursive; this just unifies Native with it.
            let recursive = RecursiveMode::Recursive;
            w.as_mut()
                .unwrap()
                .watch(&path, recursive)
                .map_err(|e| io::Error::other(e.to_string()))?;
        }

        let mut state = self.state.lock().unwrap();
        let id = state.next_id();
        state.registrations.insert(
            id,
            RegistrationState {
                // Placeholder; replaced by install_callback (or stays a no-op
                // if the caller only wants the watch registered without a stream).
                callback: Arc::new(|_| {}),
                path: arc_path.clone(),
                mode,
            },
        );
        state.path_index.entry(arc_path).or_default().push(id);
        Ok(id)
    }

    /// Install the real event callback on an existing registration.
    fn install_callback(
        &self,
        id: WatcherRegistrationId,
        cb: impl Fn(&Event) + Send + Sync + 'static,
    ) {
        if let Some(reg) = self.state.lock().unwrap().registrations.get_mut(&id) {
            reg.callback = Arc::new(cb);
        }
    }

    fn remove(&self, id: WatcherRegistrationId) {
        let mut state = self.state.lock().unwrap();
        let Some(reg) = state.registrations.remove(&id) else {
            return;
        };
        if let Some(ids) = state.path_index.get_mut(&reg.path) {
            ids.retain(|&x| x != id);
            if ids.is_empty() {
                state.path_index.remove(&reg.path);
            }
        }
        drop(state);

        let res = {
            let mut w = match reg.mode {
                WatcherMode::Native => self.native_watcher.lock().unwrap(),
                WatcherMode::Poll => self.poll_watcher.lock().unwrap(),
            };
            w.as_mut().unwrap().unwatch(&reg.path)
        };
        if let Err(e) = res {
            // inotify auto-removes a watch when its directory is deleted, so a
            // later unwatch can race that and fail benignly. Ignore WatchNotFound.
            if !matches!(e.kind, notify::ErrorKind::WatchNotFound) {
                tracing::warn!("fs watcher unwatch failed for {:?}: {e}", reg.path);
            }
        }
    }

    fn ensure_watcher(&self, mode: WatcherMode) -> notify::Result<()> {
        match mode {
            WatcherMode::Native => {
                let mut slot = self.native_watcher.lock().unwrap();
                if slot.is_some() {
                    return Ok(());
                }
                // CORE-style filtering isn't available in notify 6.1; the native
                // backend already avoids Access events by default. Use default config.
                let watcher = notify::RecommendedWatcher::new(
                    {
                        let tx = self.event_tx.clone();
                        move |event| {
                            let _ = tx.try_send((WatcherMode::Native, event));
                        }
                    },
                    notify::Config::default(),
                )?;
                *slot = Some(Box::new(watcher) as Box<dyn WatchBackend>);
            }
            WatcherMode::Poll => {
                let mut slot = self.poll_watcher.lock().unwrap();
                if slot.is_some() {
                    return Ok(());
                }
                let config = notify::Config::default().with_poll_interval(poll_interval());
                let watcher = notify::PollWatcher::new(
                    {
                        let tx = self.event_tx.clone();
                        move |event| {
                            let _ = tx.try_send((WatcherMode::Poll, event));
                        }
                    },
                    config,
                )?;
                *slot = Some(Box::new(watcher) as Box<dyn WatchBackend>);
            }
        }
        Ok(())
    }

    /// Find every registration whose watched path is an ancestor of (or equal to)
    /// `event_path`, and invoke its callback. Mirrors Zed's ancestor-matching.
    fn dispatch(&self, event: Event) {
        let mut ids: Vec<WatcherRegistrationId> = Vec::new();
        let state = self.state.lock().unwrap();
        for event_path in &event.paths {
            // `ancestors()` yields the path itself first, then each parent up to
            // the root. We must check every ancestor (including the parent dirs),
            // not stop at the path itself — otherwise a watched directory never
            // matches events on its children.
            for ancestor in event_path.ancestors() {
                if let Some(regs) = state.path_index.get(ancestor) {
                    ids.extend_from_slice(regs);
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();
        tracing::debug!(
            paths = ?event.paths,
            kind = ?event.kind,
            watched = ?state.path_index.keys().collect::<Vec<_>>(),
            matched = ?ids,
            "fs watcher dispatching event"
        );
        for id in ids {
            if let Some(reg) = state.registrations.get(&id) {
                (reg.callback)(&event);
            }
        }
    }
}

static GLOBAL_WATCHER: OnceLock<GlobalWatcher> = OnceLock::new();

fn global_watcher() -> &'static GlobalWatcher {
    GLOBAL_WATCHER.get_or_init(|| {
        let (event_tx, event_rx) = async_channel::unbounded();
        std::thread::Builder::new()
            .name("fs-watcher-dispatch".to_string())
            .spawn(move || {
                while let Ok((_mode, event)) = event_rx.recv_blocking() {
                    match event {
                        Ok(event) => global_watcher().dispatch(event),
                        Err(e) => tracing::warn!("fs watcher error: {e}"),
                    }
                }
            })
            .expect("failed to spawn fs watcher dispatch thread");
        GlobalWatcher {
            state: Mutex::new(WatcherState {
                registrations: HashMap::new(),
                last_id: WatcherRegistrationId(0),
                path_index: HashMap::new(),
            }),
            native_watcher: Mutex::new(None),
            poll_watcher: Mutex::new(None),
            event_tx,
        }
    })
}
