use config_rs::root::base_dir;
use futures::{stream, Stream, StreamExt};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

#[fnrpc::rpc_subscribe]
pub fn watch_task_log(task_dir: String) -> impl Stream<Item = String> {
    let p = if Path::new(&task_dir).is_relative() {
        base_dir().join(&task_dir)
    } else {
        Path::new(&task_dir).to_path_buf()
    };
    let task_id = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let log_path = p.join(format!("{task_id}.log"));
    // Watch the *parent* directory (NonRecursive) rather than the log file itself.
    // inotify (and most OS watchers) drop a watch when the watched file is
    // truncated/recreated; watching the parent survives that and we filter by name.
    let watch_dir = p
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| p.clone());
    let log_file_name = format!("{task_id}.log");

    let (initial_lines, initial_len) = match std::fs::read_to_string(&log_path) {
        Ok(c) => {
            let all_lines: Vec<&str> = c.lines().collect();
            let tail = if all_lines.len() > 50 {
                all_lines[all_lines.len() - 50..].to_vec()
            } else {
                all_lines.clone()
            };
            (tail.into_iter().map(String::from).collect(), c.len() as u64)
        }
        Err(_) => (vec![], 0),
    };

    struct State {
        log_path: PathBuf,
        log_file_name: String,
        last_len: u64,
        tail: std::vec::IntoIter<String>,
        event_stream: std::pin::Pin<Box<dyn Stream<Item = fs::PathEvent> + Send>>,
    }

    // If we can't watch the directory, fall back to a stream that just yields the
    // initial tail and then ends (no live updates).
    let event_stream: std::pin::Pin<Box<dyn Stream<Item = fs::PathEvent> + Send>> =
        match fs::watch_stream(watch_dir.clone()) {
            Ok(s) => Box::pin(s),
            Err(e) => {
                tracing::error!("failed to watch task log dir {watch_dir:?}: {e}");
                Box::pin(stream::empty())
            }
        };

    stream::unfold(
        State {
            log_path,
            log_file_name,
            last_len: initial_len,
            tail: initial_lines.into_iter(),
            event_stream,
        },
        |mut state| async move {
            loop {
                if let Some(line) = state.tail.next() {
                    return Some((line, state));
                }
                // Wait for a filesystem event on the watched directory.
                let event = match state.event_stream.next().await {
                    Some(e) => e,
                    None => return None,
                };
                // Ignore events for files other than our `.log`.
                let relevant = event
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|name| name == state.log_file_name);
                if !relevant {
                    continue;
                }
                if let Ok(meta) = tokio::fs::metadata(&state.log_path).await {
                    let len = meta.len();
                    if len >= state.last_len {
                        // File grew (or unchanged): read the appended bytes.
                        if let Ok(mut f) = tokio::fs::File::open(&state.log_path).await {
                            if f.seek(std::io::SeekFrom::Start(state.last_len))
                                .await
                                .is_ok()
                            {
                                let mut content = String::new();
                                if f.read_to_string(&mut content).await.is_ok()
                                    && !content.is_empty()
                                {
                                    state.last_len = len;
                                    state.tail = content
                                        .lines()
                                        .map(String::from)
                                        .collect::<Vec<_>>()
                                        .into_iter();
                                    if let Some(line) = state.tail.next() {
                                        return Some((line, state));
                                    }
                                }
                            }
                        }
                    } else {
                        // File shrank (truncated/recreated): re-read from the start.
                        if let Ok(c) = tokio::fs::read_to_string(&state.log_path).await {
                            state.last_len = c.len() as u64;
                            state.tail =
                                c.lines().map(String::from).collect::<Vec<_>>().into_iter();
                            if let Some(line) = state.tail.next() {
                                return Some((line, state));
                            }
                        }
                    }
                }
            }
        },
    )
}
