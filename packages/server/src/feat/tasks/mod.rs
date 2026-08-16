pub mod log;
pub mod tree;

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use config_rs::{
    root::base_dir,
    // servers::ServerType
};

use ld_core::{
    cmd::tasks::get_task::GroupInfo,
    context::{
        self,
        TaskCtx,
        // Task
    },
    input::Input,
};

/// 各任务目录是否已有续跑在途 (防同一任务并发续跑)。
///
/// 锁的获取/释放都在 `spawn_blocking` 闭包内 (RAII), 即使客户端断连导致外层
/// `.await` 被取消, 闭包仍会跑完并释放锁, 不会泄漏。
static INFLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn inflight_lock() -> &'static Mutex<HashSet<String>> {
    INFLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

#[fnrpc::rpc_query]
pub async fn get_group_list() -> Result<Vec<GroupInfo>, String> {
    ld_core::cmd::tasks::get_task::get_group_list()
}

#[fnrpc::rpc_query]
pub async fn get_task_ctx(task_dir: String) -> Result<TaskCtx, String> {
    let path = base_dir().join(&task_dir);
    context::read_ctx(
        &path
            .to_str()
            .ok_or_else(|| format!("Invalid task_dir: {}", task_dir))?,
    )
}

#[fnrpc::rpc_mutate]
pub async fn continue_task(task_dir: String, from_stage: String) -> Result<(), String> {
    let abs_task_dir = base_dir().join(&task_dir);
    let abs_task_dir_str = abs_task_dir
        .to_str()
        .ok_or_else(|| "invalid task_dir".to_string())?
        .to_string();

    // 读 ctx.json 的 input 字段作为续跑基准配置 (仅改写 task 相关字段, 其余原样保留)。
    let ctx_path = abs_task_dir.join("ctx.json");
    let ctx_raw =
        std::fs::read_to_string(&ctx_path).map_err(|e| format!("read ctx.json failed: {}", e))?;
    let mut ctx: serde_json::Value =
        serde_json::from_str(&ctx_raw).map_err(|e| format!("parse ctx.json failed: {}", e))?;

    let mut input_value = ctx
        .get_mut("input")
        .map(|v| v.take())
        .ok_or_else(|| "ctx.json 缺少 input 字段".to_string())?;
    let task = input_value
        .get_mut("task")
        .ok_or_else(|| "input 缺少 task 字段".to_string())?;
    task["taskDir"] = serde_json::Value::String(abs_task_dir_str.clone());
    task["action"] = serde_json::Value::String("continue".into());
    task["continueFrom"] = serde_json::Value::String(from_stage);

    let input: Input =
        serde_json::from_value(input_value).map_err(|e| format!("parse input failed: {}", e))?;

    // 在 blocking 池跑 Rust pipeline (ld_core::cmd::tasks::continue_task:
    // setCtx 合并 → continue_pipeline)。不再 spawn bun / 写全局 input.json /
    // 硬编码 runtime —— 那些是旧 TS 实现的遗留。
    let task_dir_owned = abs_task_dir_str.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut set = inflight_lock().lock().unwrap();
        if !set.insert(task_dir_owned.clone()) {
            return Err("任务已在续跑中".to_string());
        }
        let r = ld_core::cmd::tasks::continue_task(&input);
        set.remove(&task_dir_owned);
        r.map_err(|e| format!("continue_task 失败: {e:#}"))
    })
    .await;

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("continue_task 任务崩溃: {e}")),
    }
}

/// 启动新任务 (右上角「+」弹窗)。
///
/// 用服务器端 base 配置构造 Input (镜像 input.jsonc 的已知可用配置), 避免
/// `Stages::default()` 的 pytorch/cuda 默认值在无 torch server 时失败。
/// 返回相对 `workfolder` 的 task_dir, 供前端跳转任务页。
#[fnrpc::rpc_mutate]
pub async fn start_task(url: String) -> Result<String, String> {
    let base = serde_json::json!({
        "command": "task",
        "task": {
            "action": "start",
            "url": url,
            "pipeline": "dub",
            "subtitleSource": "sf_ocr",
        },
        "stages": {
            "separate": {"runtime": "burn-tch", "device": "cpu", "always": true},
            "asr": {"runtime": "ggml", "device": "vulkan", "useSeparated": true,
                    "mixMode": "sidechain", "vad": true, "vadModel": "silero-v6", "wordsOutput": true},
            "asr_ocr": {"runtime": "ort-rust", "textScore": 0.45},
            "asr_ocr_fix": {"is_resample": false, "llmFix": true},
            "asr_fix": {"llmFix": true},
            "sf_ocr_fix": {"llmFix": true},
            "translate": {"enabled": true},
            "split_audio": {"startPadMs": 100, "endPadMs": 0, "vadAlign": false},
            "tts": {"runtime": "cloud", "device": "cpu"},
            "mix_audio": {"maxSpeed": 1.55, "maxAdvanceMs": 300, "maxDelayMs": 300},
            "mix_video": {"fontSize": 21.4, "marginV": 45,
                          "font": "Noto Sans CJK SC Medium", "shadow": 1.1, "bgmGain": -9},
        }
    });

    let input: Input =
        serde_json::from_value(base).map_err(|e| format!("parse input failed: {}", e))?;

    // 只跑导入 (拷贝/下载 + 探测 + 写 ctx.json), 拿到 task_dir 立即返回;
    // 完整 pipeline 在后台跑, 前端跳转任务页后由 ctx watcher 实时刷新 stage 徽章。
    let ctx = tokio::task::spawn_blocking(move || {
        ld_core::tasks::import::download::import_video(&input)
            .map_err(|e| format!("import_video 失败: {e:#}"))
    })
    .await;

    let abs_task_dir = match ctx {
        Ok(Ok(ctx)) => ctx.task.task_dir,
        Ok(Err(e)) => return Err(e),
        Err(e) => return Err(format!("import 任务崩溃: {e}")),
    };

    // 后台跑完整 pipeline (不阻塞 RPC); 失败仅记日志, 任务页可续跑。
    let task_dir = abs_task_dir.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = ld_core::tasks::pipeline::run_pipeline(&task_dir) {
            tracing::error!("start_task pipeline 失败 ({task_dir}): {e:#}");
        }
    });

    // 返回相对 workfolder 的 task_dir (供前端导航 /group/<group>/<task>)。
    std::path::Path::new(&abs_task_dir)
        .strip_prefix(base_dir())
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|_| format!("task_dir 不在 workfolder 内: {abs_task_dir}"))
}
