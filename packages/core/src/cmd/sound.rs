//! 任务提示音播放 (迁移自 cli/main.rs, 镜像 TS `playWav`)。
//!
//! 用 ffplay 播放, headless / 无 ffplay 环境播放失败属正常, 静默忽略。

use std::process::Command;

use config_rs::path::models::{command_done_path, task_fail_path, task_success_path};

/// 用 ffplay 播放提示音: `-nodisp -autoexit`, 失败静默不中断流程。
///
/// 文件不存在或 ffplay 不在 PATH 时直接返回, 不报错 (镜像 TS `playWav`:
/// `spawnSync('ffplay', ['-nodisp','-autoexit', path], { stdio: 'ignore' })`)。
pub fn play_wav(path: &std::path::Path) {
    if !path.exists() {
        return;
    }
    let _ = Command::new("ffplay")
        .args(["-nodisp", "-autoexit"])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// 播放任务成功提示音。
pub fn play_task_success() {
    play_wav(&task_success_path());
}

/// 播放任务失败提示音。
pub fn play_task_fail() {
    play_wav(&task_fail_path());
}

/// 播放命令完成提示音 (区别于任务完成: enqueue 提交/servers 管理等)。
pub fn play_command_done() {
    play_wav(&command_done_path());
}

/// 后台播放提示音 (fire-and-forget): 播放会阻塞到音频结束,
/// 队列 worker 等 async 上下文用此避免卡住循环。
pub fn play_task_success_bg() {
    std::thread::spawn(play_task_success);
}

/// 后台播放任务失败提示音。
pub fn play_task_fail_bg() {
    std::thread::spawn(play_task_fail);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_wav_missing_path_is_silent() {
        // 不存在的路径应静默返回, 不 panic。
        play_wav(&std::path::Path::new("/nonexistent/nope.wav"));
    }

    #[test]
    fn sound_paths_structure() {
        let s = task_success_path();
        assert!(
            s.to_string_lossy().ends_with("assets/media/task_success.wav"),
            "got {s:?}"
        );
        let f = task_fail_path();
        assert!(
            f.to_string_lossy().ends_with("assets/media/error.wav"),
            "got {f:?}"
        );
    }
}
