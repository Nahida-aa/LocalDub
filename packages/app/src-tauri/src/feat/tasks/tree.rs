use config_rs::root::base_dir;
use futures::{stream, Stream};
use std::path::Path;
use std::pin::Pin;

/// Subscribe to real-time changes in a task (episode-level) directory tree.
///
/// Watches the directory at `task_dir` (relative to `base_dir()`, or an absolute
/// path) with a **recursive** OS watcher, so it covers the whole subtree — including
/// subdirectories created *after* the watch starts (e.g. a pipeline's `merge_video/`
/// appearing mid-run and the `.srt`/video files inside it). Emits [`fs::PathEvent`]
/// for every leaf that changes anywhere under `task_dir` (e.g. `.log` updates,
/// generated `.srt`/video). The event carries the path and the kind of change, but
/// **never the file contents**; the consumer decides when to read.
/// Core stream-building logic, free of the `#[fnrpc::rpc_subscribe]` macro so it
/// can be unit-tested directly. Resolves `task_dir` (relative to `base_dir()`, or
/// an absolute path) and returns a watch stream, or an empty stream on failure.
fn build_tree_stream(task_dir: String) -> Pin<Box<dyn Stream<Item = fs::PathEvent> + Send>> {
    let p = if Path::new(&task_dir).is_relative() {
        base_dir().join(&task_dir)
    } else {
        Path::new(&task_dir).to_path_buf()
    };

    match fs::watch_stream(p) {
        Ok(s) => Box::pin(s),
        Err(e) => {
            tracing::error!("failed to watch task tree {task_dir}: {e}");
            Box::pin(stream::empty())
        }
    }
}

#[fnrpc::rpc_subscribe]
pub fn watch_task_tree(task_dir: String) -> impl Stream<Item = fs::PathEvent> {
    build_tree_stream(task_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use std::pin::Pin;

    #[tokio::test]
    async fn accepts_absolute_dir_without_panic() {
        // An absolute, existing directory should yield a watchable stream.
        // We don't assert on real OS events here (that belongs to the `fs` crate
        // integration tests); we only verify the stream wires up without
        // panicking and is consumable.
        let dir = std::env::temp_dir().join(format!("localdub_tree_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");

        let mut stream: Pin<Box<dyn Stream<Item = fs::PathEvent>>> =
            build_tree_stream(dir.to_string_lossy().to_string());
        // Drain at most one item; the stream must be polled without panicking.
        let _ = tokio::time::timeout(std::time::Duration::from_millis(200), stream.next()).await;

        let _ = std::fs::remove_dir(&dir);
    }

    #[tokio::test]
    async fn relative_path_resolves_via_base_dir() {
        // A relative path must not be passed straight to the OS watcher; it
        // should be resolved (joined onto base_dir). We just assert the call
        // returns a stream object and is consumable without panicking.
        let mut stream: Pin<Box<dyn Stream<Item = fs::PathEvent>>> =
            build_tree_stream("workfolder".to_string());
        let _ = tokio::time::timeout(std::time::Duration::from_millis(200), stream.next()).await;
    }
}
