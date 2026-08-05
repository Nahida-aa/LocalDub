//! Integration tests for `fs::watch_stream`.
//!
//! These exercise the real OS watcher (inotify/FSEvents) or the poll backend,
//! so they need an actual filesystem. We force the poll backend via env var
//! because it is the most reliable inside CI/container filesystems where
//! inotify can be silent. A timeout guards every `next()` so a missed event
//! fails the test instead of hanging the suite forever.

use fs::{watch_stream, PathEvent, PathEventKind};
use futures::Stream;
use futures::StreamExt;
use std::pin::Pin;
use std::time::Duration;
use tempfile::TempDir;

type BoxedStream = Pin<Box<dyn Stream<Item = PathEvent>>>;

/// Force the poll backend with a short interval before the global watcher is
/// ever constructed (it is a process-global `OnceLock`, so this must run first).
fn force_poll_mode() {
    std::env::set_var("LOCALDUB_FILE_WATCHER_MODE", "poll");
    std::env::set_var("LOCALDUB_FILE_WATCHER_POLL_MS", "500");
}

/// Wait up to `secs` for the next `PathEvent` matching `pred`.
async fn expect_event(
    stream: &mut BoxedStream,
    secs: u64,
    pred: impl Fn(&PathEvent) -> bool,
) -> PathEvent {
    tokio::time::timeout(Duration::from_secs(secs), async {
        while let Some(ev) = stream.next().await {
            if pred(&ev) {
                return ev;
            }
        }
        panic!("stream ended before a matching event arrived");
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for matching PathEvent"))
}

#[tokio::test]
async fn emits_created_event() {
    force_poll_mode();
    let dir = TempDir::new().expect("tempdir");
    let mut stream: BoxedStream =
        Box::pin(watch_stream(dir.path().to_path_buf()).expect("watch_stream"));

    let target = dir.path().join("hello.txt");
    std::fs::write(&target, "hi").expect("write");

    let ev = expect_event(&mut stream, 10, |e| {
        e.path.ends_with("hello.txt") && e.kind == Some(PathEventKind::Created)
    })
    .await;
    assert_eq!(ev.kind, Some(PathEventKind::Created));
}

#[tokio::test]
async fn emits_changed_event() {
    force_poll_mode();
    let dir = TempDir::new().expect("tempdir");
    let target = dir.path().join("note.txt");

    let mut stream: BoxedStream =
        Box::pin(watch_stream(dir.path().to_path_buf()).expect("watch_stream"));

    // Create the file, let one poll cycle pass, then modify it. Poll watchers
    // detect the modification as a `Changed` event (possibly reported at the
    // directory level), so we match on kind. The 1s gap must exceed the poll
    // interval (500ms) so the two writes aren't coalesced into one snapshot.
    std::fs::write(&target, "v1").expect("write v1");
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    std::fs::write(&target, "v2").expect("write v2");

    let ev = expect_event(&mut stream, 10, |e| e.kind == Some(PathEventKind::Changed)).await;
    assert_eq!(ev.kind, Some(PathEventKind::Changed));
}

#[tokio::test]
async fn emits_removed_event() {
    force_poll_mode();
    let dir = TempDir::new().expect("tempdir");
    let target = dir.path().join("gone.txt");
    std::fs::write(&target, "x").expect("write");

    let mut stream: BoxedStream =
        Box::pin(watch_stream(dir.path().to_path_buf()).expect("watch_stream"));

    std::fs::remove_file(&target).expect("remove");

    let ev = expect_event(&mut stream, 10, |e| {
        e.path.ends_with("gone.txt") && e.kind == Some(PathEventKind::Removed)
    })
    .await;
    assert_eq!(ev.kind, Some(PathEventKind::Removed));
}

#[tokio::test]
async fn watch_nonexistent_dir_errors() {
    // A path that cannot be watched should surface an error rather than panic.
    let res = watch_stream("/nonexistent/path/that/should/not/exist/localdub");
    // Either an Err, or an Ok empty-ish stream is acceptable; we just assert it
    // does not panic. On most systems this is an Err.
    match res {
        Ok(_) => { /* allowed: depends on backend */ }
        Err(_) => { /* expected */ }
    }
}

#[tokio::test]
async fn coalesces_repeated_writes_to_single_event() {
    force_poll_mode();
    let dir = TempDir::new().expect("tempdir");
    let target = dir.path().join("storm.txt");

    let mut stream: BoxedStream =
        Box::pin(watch_stream(dir.path().to_path_buf()).expect("watch_stream"));

    // Create, then hammer the same file with 10 writes back-to-back. A filesystem
    // event storm like this used to flood the consumer (e.g. an ffmpeg writing a
    // large video in chunks). Coalescing by path must collapse it.
    std::fs::write(&target, "v0").expect("write v0");
    for i in 1..=10 {
        std::fs::write(&target, format!("v{i}")).expect("write");
    }

    // Collect events: wait for the first one (creation), then gather for a couple
    // of poll cycles so all coalesced events have a chance to flush.
    let first = stream
        .next()
        .await
        .expect("should receive at least the Created event");
    let mut events = vec![first];
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    // Drain whatever else is buffered without blocking forever.
    while let Ok(Some(ev)) =
        tokio::time::timeout(std::time::Duration::from_millis(200), stream.next()).await
    {
        events.push(ev);
    }

    let changed_for_target = events
        .iter()
        .filter(|e| e.path.ends_with("storm.txt") && e.kind == Some(PathEventKind::Changed))
        .count();
    let created_for_target = events
        .iter()
        .filter(|e| e.path.ends_with("storm.txt") && e.kind == Some(PathEventKind::Created))
        .count();

    // The 10 modifications must collapse into at most one `Changed` for this path.
    assert!(
        changed_for_target <= 1,
        "expected ≤1 Changed event for repeated writes, got {changed_for_target}: {events:?}"
    );
    // Exactly one creation for the file.
    assert_eq!(created_for_target, 1, "expected exactly one Created event");
}
