use std::process::{ExitStatus, Stdio};

/// 同步执行命令, 超时 kill 视为空输出 (镜像 TS `execSync(..., {timeout})` catch → '')。
///
/// 注意: stdout 用独立线程边跑边读, 避免输出 >64KB 时阻塞子进程写满 pipe buffer
/// (vulkaninfo 全量文本 ~200KB, 若 wait 后才读会挂起)。
pub fn run_with_timeout(cmd: &str, args: &[&str], timeout_ms: u64) -> String {
    let mut c = std::process::Command::new(cmd);
    c.args(args);
    c.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = match c.spawn() {
        Ok(child) => child,
        Err(_) => return String::new(),
    };
    let mut stdout_handle = child.stdout.take();
    // 读线程: 把 stdout 管道读空 (子进程可尽情写), 主线程负责 wait/kill。
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = String::new();
        let _ = stdout_handle.as_mut().map(|h| h.read_to_string(&mut buf));
        buf
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let mut status: Option<ExitStatus> = None;
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(s)) => {
                status = Some(s);
                break;
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }
    // 子进程已退出/被 kill, 读线程会因 EOF 结束, join 收集。
    let stdout = match reader.join() {
        Ok(s) => s,
        Err(_) => String::new(),
    };
    match status {
        Some(s) if s.success() => stdout.trim().to_string(),
        // 超时已 kill → 对齐 TS catch → 空串
        _ => {
            let _ = timed_out;
            String::new()
        }
    }
}