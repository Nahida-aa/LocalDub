//! 同步子进程执行辅助 (超时 kill, 捕获输出)。

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 同步执行命令, 轮询 `try_wait`, 超时 kill。
///
/// 捕获 stdout (trim) 与 stderr; `capture_stderr=false` 时 stderr 重定向到 `/dev/null`。
///
/// 返回 `(exit_success, stdout_trim, stderr_trim)`:
/// - `status.success()` 决定第一个布尔 (非零退出也是 Some(out), 与 TS `spawnSync` trim 语义一致);
/// - spawn 失败 / 超时 kill → `(false, "", "")`。
pub fn run_cmd<I, S>(
    cmd: &str,
    args: I,
    cwd: Option<&Path>,
    timeout: Duration,
    capture_stderr: bool,
) -> (bool, String, String)
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    use std::io::Read;

    let mut c = Command::new(cmd);
    c.args(args);
    if let Some(dir) = cwd {
        c.current_dir(dir);
    }
    c.stdout(Stdio::piped());
    c.stderr(if capture_stderr {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut child = match c.spawn() {
        Ok(child) => child,
        Err(_) => return (false, String::new(), String::new()),
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                let _ = child.stdout.take().map(|mut h| h.read_to_string(&mut stdout));
                if capture_stderr {
                    let _ = child.stderr.take().map(|mut h| h.read_to_string(&mut stderr));
                }
                return (
                    status.success(),
                    stdout.trim().to_string(),
                    stderr.trim().to_string(),
                );
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return (false, String::new(), String::new());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return (false, String::new(), String::new()),
        }
    }
}