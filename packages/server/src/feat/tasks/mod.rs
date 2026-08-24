pub mod log;
pub mod queue;
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
    eprintln!(
        "[continue_task] base_dir={} task_dir={task_dir} abs={abs_task_dir_str}",
        base_dir().display()
    );

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

/// 重新生成指定 TTS 段 (续跑模式: continueFrom=tts + stages.tts.regenIndices)。
///
/// `continue_run=true` 时重生成后继续跑完整个 pipeline (镜像 input.jsonc 手工改
/// `regenIndices` 后「从 tts 继续运行」); `false` 时只重生成, 到 tts 阶段结束即停
/// (targetStage=tts, 供先听效果再手动继续)。
#[fnrpc::rpc_mutate]
pub async fn regen_tts(
    task_dir: String,
    seg_indices: Vec<u32>,
    continue_run: bool,
) -> Result<(), String> {
    let abs_task_dir = base_dir().join(&task_dir);
    let abs_task_dir_str = abs_task_dir
        .to_str()
        .ok_or_else(|| "invalid task_dir".to_string())?
        .to_string();

    // 读 ctx.json 的 input 字段作为续跑基准配置 (仅改写 task / stages.tts, 其余保留)。
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
    task["continueFrom"] = serde_json::Value::String("tts".into());
    if continue_run {
        // 继续跑完整个 pipeline: 清掉可能残留的 targetStage, 不中途停止。
        task["targetStage"] = serde_json::Value::Null;
    } else {
        // 只重生成: 跑到 tts 阶段结束即停。
        task["targetStage"] = serde_json::Value::String("tts".into());
    }

    // 合并 regenIndices 到 stages.tts (镜像手工在 input.jsonc 里配置 regenIndices)。
    match input_value["stages"].as_object_mut() {
        Some(stages_obj) => {
            let tts = stages_obj
                .entry("tts".to_string())
                .or_insert_with(|| serde_json::json!({}));
            tts["regenIndices"] = serde_json::json!(seg_indices);
        }
        None => {
            input_value["stages"] = serde_json::json!({ "tts": { "regenIndices": seg_indices } });
        }
    }

    let input: Input =
        serde_json::from_value(input_value).map_err(|e| format!("parse input failed: {}", e))?;

    // 与 continue_task 共用同一把 in-flight 锁, 防同一任务并发续跑。
    let task_dir_owned = abs_task_dir_str.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut set = inflight_lock().lock().unwrap();
        if !set.insert(task_dir_owned.clone()) {
            return Err("任务已在续跑中".to_string());
        }
        let r = ld_core::cmd::tasks::continue_task(&input);
        set.remove(&task_dir_owned);
        r.map_err(|e| format!("regen_tts 失败: {e:#}"))
    })
    .await;

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("regen_tts 任务崩溃: {e}")),
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

// --- 任务队列 (CLI 通过 fnrpc 入队, worker 串行执行) ---

use crate::ctx::Ctx;
use crate::feat::tasks::queue::QueueEntry;

/// 加入一个新任务 (start): 入队完整 input (action=start), worker 串行执行
/// (import 视频 + 完整 pipeline)。返回队列 ID。
#[fnrpc::rpc_mutate]
pub async fn enqueue_start(ctx: &Ctx, input: Input) -> Result<u64, String> {
    use ld_core::tasks::args::TaskAction;
    if input.task.as_ref().and_then(|t| t.action) != Some(TaskAction::Start) {
        return Err("enqueue_start 需要 input.task.action = start".to_string());
    }
    Ok(ctx.state.queue.enqueue(input))
}

/// 加入一个续跑任务 (continue): 入队完整 input (action=continue), worker 串行执行
/// (续跑已有任务)。返回队列 ID。
#[fnrpc::rpc_mutate]
pub async fn enqueue_continue(ctx: &Ctx, input: Input) -> Result<u64, String> {
    use ld_core::tasks::args::TaskAction;
    if input.task.as_ref().and_then(|t| t.action) != Some(TaskAction::Continue) {
        return Err("enqueue_continue 需要 input.task.action = continue".to_string());
    }
    Ok(ctx.state.queue.enqueue(input))
}

/// 列出队列中的任务 (含状态)。
#[fnrpc::rpc_query]
pub async fn list_queue(ctx: &Ctx) -> Vec<QueueEntry> {
    ctx.state.queue.snapshot()
}

/// 取消一个待执行任务 (仅 queued 状态可取消)。
#[fnrpc::rpc_mutate]
pub async fn cancel_queue(ctx: &Ctx, id: u64) -> Result<bool, String> {
    Ok(ctx.state.queue.cancel(id))
}
