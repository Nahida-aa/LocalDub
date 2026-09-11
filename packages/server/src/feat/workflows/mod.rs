pub mod log;
pub mod queue;
pub mod tree;

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use config_rs::{
    root::repo_root,
    // servers::ServerType
};

use ld_core::{
    cmd::workflows::{
        enqueue_dir::{DirScanOutput, scan_dir_videos},
        get_workflow::{EnqueueDirResult, GroupInfo},
    },
    context::{
        self,
        WorkflowCtx,
        // Workflow
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
    ld_core::cmd::workflows::get_workflow::get_group_list()
}

#[fnrpc::rpc_query]
pub async fn get_workflow_ctx(workflow_dir: String) -> Result<WorkflowCtx, String> {
    let path = repo_root().join(&workflow_dir);
    context::read_ctx(
        &path
            .to_str()
            .ok_or_else(|| format!("Invalid workflow_dir: {}", workflow_dir))?,
    )
}

#[fnrpc::rpc_mutate]
pub async fn continue_workflow(workflow_dir: String, from_step: String) -> Result<(), String> {
    let abs_workflow_dir = repo_root().join(&workflow_dir);
    let abs_workflow_dir_str = abs_workflow_dir
        .to_str()
        .ok_or_else(|| "invalid workflow_dir".to_string())?
        .to_string();
    eprintln!(
        "[continue_workflow] base_dir={} workflow_dir={workflow_dir} abs={abs_workflow_dir_str}",
        repo_root().display()
    );

    // 读 ctx.json 的 input 字段作为续跑基准配置 (仅改写 workflow 相关字段, 其余原样保留)。
    let ctx_path = abs_workflow_dir.join("ctx.json");
    let ctx_raw =
        std::fs::read_to_string(&ctx_path).map_err(|e| format!("read ctx.json failed: {}", e))?;
    let mut ctx: serde_json::Value =
        serde_json::from_str(&ctx_raw).map_err(|e| format!("parse ctx.json failed: {}", e))?;

    let mut input_value = ctx
        .get_mut("input")
        .map(|v| v.take())
        .ok_or_else(|| "ctx.json 缺少 input 字段".to_string())?;
    let workflow = input_value
        .get_mut("workflow")
        .ok_or_else(|| "input 缺少 workflow 字段".to_string())?;
    workflow["videoDir"] = serde_json::Value::String(abs_workflow_dir_str.clone());
    workflow["action"] = serde_json::Value::String("continue".into());
    workflow["continueFrom"] = serde_json::Value::String(from_step);

    let input: Input =
        serde_json::from_value(input_value).map_err(|e| format!("parse input failed: {}", e))?;

    // 在 blocking 池跑 Rust pipeline (ld_core::cmd::workflows::continue_workflow:
    // setCtx 合并 → continue_pipeline)。不再 spawn bun / 写全局 input.json /
    // 硬编码 runtime —— 那些是旧 TS 实现的遗留。
    let workflow_dir_owned = abs_workflow_dir_str.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut set = inflight_lock().lock().unwrap();
        if !set.insert(workflow_dir_owned.clone()) {
            return Err("任务已在续跑中".to_string());
        }
        let r = ld_core::cmd::workflows::continue_workflow(&input);
        set.remove(&workflow_dir_owned);
        r.map_err(|e| format!("continue_workflow 失败: {e:#}"))
    })
    .await;

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("continue_workflow 任务崩溃: {e}")),
    }
}

/// 重新生成指定 TTS 段 (续跑模式: continueFrom=tts + steps.tts.regenIndices)。
///
/// `continue_run=true` 时重生成后继续跑完整个 pipeline (镜像 input.jsonc 手工改
/// `regenIndices` 后「从 tts 继续运行」); `false` 时只重生成, 到 tts 阶段结束即停
/// (targetStep=tts, 供先听效果再手动继续)。
#[fnrpc::rpc_mutate]
pub async fn regen_tts(
    workflow_dir: String,
    seg_indices: Vec<u32>,
    continue_run: bool,
) -> Result<(), String> {
    let abs_workflow_dir = repo_root().join(&workflow_dir);
    let abs_workflow_dir_str = abs_workflow_dir
        .to_str()
        .ok_or_else(|| "invalid workflow_dir".to_string())?
        .to_string();

    // 读 ctx.json 的 input 字段作为续跑基准配置 (仅改写 workflow / steps.tts, 其余保留)。
    let ctx_path = abs_workflow_dir.join("ctx.json");
    let ctx_raw =
        std::fs::read_to_string(&ctx_path).map_err(|e| format!("read ctx.json failed: {}", e))?;
    let mut ctx: serde_json::Value =
        serde_json::from_str(&ctx_raw).map_err(|e| format!("parse ctx.json failed: {}", e))?;

    let mut input_value = ctx
        .get_mut("input")
        .map(|v| v.take())
        .ok_or_else(|| "ctx.json 缺少 input 字段".to_string())?;
    let workflow = input_value
        .get_mut("workflow")
        .ok_or_else(|| "input 缺少 workflow 字段".to_string())?;
    workflow["videoDir"] = serde_json::Value::String(abs_workflow_dir_str.clone());
    workflow["action"] = serde_json::Value::String("continue".into());
    workflow["continueFrom"] = serde_json::Value::String("tts".into());
    if continue_run {
        // 继续跑完整个 pipeline: 清掉可能残留的 targetStep, 不中途停止。
        workflow["targetStep"] = serde_json::Value::Null;
    } else {
        // 只重生成: 跑到 tts 阶段结束即停。
        workflow["targetStep"] = serde_json::Value::String("tts".into());
    }

    // 合并 regenIndices 到 steps.tts (镜像手工在 input.jsonc 里配置 regenIndices)。
    match input_value["steps"].as_object_mut() {
        Some(steps_obj) => {
            let tts = steps_obj
                .entry("tts".to_string())
                .or_insert_with(|| serde_json::json!({}));
            tts["regenIndices"] = serde_json::json!(seg_indices);
        }
        None => {
            input_value["steps"] = serde_json::json!({ "tts": { "regenIndices": seg_indices } });
        }
    }

    let input: Input =
        serde_json::from_value(input_value).map_err(|e| format!("parse input failed: {}", e))?;

    // 与 continue_workflow 共用同一把 in-flight 锁, 防同一任务并发续跑。
    let workflow_dir_owned = abs_workflow_dir_str.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut set = inflight_lock().lock().unwrap();
        if !set.insert(workflow_dir_owned.clone()) {
            return Err("任务已在续跑中".to_string());
        }
        let r = ld_core::cmd::workflows::continue_workflow(&input);
        set.remove(&workflow_dir_owned);
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
/// `Steps::default()` 的 pytorch/cuda 默认值在无 torch server 时失败。
/// 返回相对 `workfolder` 的 workflow_dir, 供前端跳转任务页。
#[fnrpc::rpc_mutate]
pub async fn start_workflow(url: String) -> Result<String, String> {
    let base = serde_json::json!({
        "command": "workflow",
        "workflow": {
            "action": "start",
            "url": url,
            "pipeline": "dub",
            "subtitleSource": "sf_ocr",
        },
        "steps": {
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

    // 只跑导入 (拷贝/下载 + 探测 + 写 ctx.json), 拿到 workflow_dir 立即返回;
    // 完整 pipeline 在后台跑, 前端跳转任务页后由 ctx watcher 实时刷新 step 徽章。
    let ctx = tokio::task::spawn_blocking(move || {
        ld_core::workflows::import::download::import_video(&input)
            .map_err(|e| format!("import_video 失败: {e:#}"))
    })
    .await;

    let abs_workflow_dir = match ctx {
        Ok(Ok(ctx)) => ctx.workflow.workflow_dir,
        Ok(Err(e)) => return Err(e),
        Err(e) => return Err(format!("import 任务崩溃: {e}")),
    };

    // 后台跑完整 pipeline (不阻塞 RPC); 失败仅记日志, 任务页可续跑。
    let workflow_dir = abs_workflow_dir.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = ld_core::workflows::pipeline::run_pipeline(&workflow_dir) {
            tracing::error!("start_workflow pipeline 失败 ({workflow_dir}): {e:#}");
        }
    });

    // 返回相对 workfolder 的 workflow_dir (供前端导航 /group/<group>/<workflow>)。
    std::path::Path::new(&abs_workflow_dir)
        .strip_prefix(repo_root())
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|_| format!("workflow_dir 不在 workfolder 内: {abs_workflow_dir}"))
}

// --- 任务队列 (CLI 通过 fnrpc 入队, worker 串行执行) ---

use crate::ctx::Ctx;
use crate::feat::workflows::queue::QueueEntry;

/// 加入一个新任务 (start): 入队完整 input (action=start), worker 串行执行
/// (import 视频 + 完整 pipeline)。返回队列 ID。
#[fnrpc::rpc_mutate]
pub async fn enqueue_start(ctx: &Ctx, input: Input) -> Result<u64, String> {
    use ld_core::workflows::args::WorkflowAction;
    if input.workflow.as_ref().and_then(|t| t.action) != Some(WorkflowAction::Start) {
        return Err("enqueue_start 需要 input.workflow.action = start".to_string());
    }
    Ok(ctx.state.queue.enqueue(input))
}

/// 加入一个续跑任务 (continue): 入队完整 input (action=continue), worker 串行执行
/// (续跑已有任务)。返回队列 ID。
#[fnrpc::rpc_mutate]
pub async fn enqueue_continue(ctx: &Ctx, input: Input) -> Result<u64, String> {
    use ld_core::workflows::args::WorkflowAction;
    if input.workflow.as_ref().and_then(|t| t.action) != Some(WorkflowAction::Continue) {
        return Err("enqueue_continue 需要 input.workflow.action = continue".to_string());
    }
    Ok(ctx.state.queue.enqueue(input))
}

/// 入队一个"只导入"任务 (不跑 pipeline): 批量先导入, 之后用 continue 续跑。
#[fnrpc::rpc_mutate]
pub async fn enqueue_import(ctx: &Ctx, input: Input) -> Result<u64, String> {
    use ld_core::workflows::args::WorkflowAction;
    if input.workflow.as_ref().and_then(|t| t.action) != Some(WorkflowAction::Import) {
        return Err("enqueue_import 需要 input.workflow.action = import".to_string());
    }
    Ok(ctx.state.queue.enqueue(input))
}

/// 列出队列中的任务 (含状态)。
#[fnrpc::rpc_query]
pub async fn list_queue(ctx: &Ctx) -> Vec<QueueEntry> {
    ctx.state.queue.snapshot()
}

/// 批量入队一个本地目录: 扫描顶层视频文件, 每个去重后入队一条 action=start 任务。
/// 返回摘要 (扫描/入队/跳过计数)。
#[fnrpc::rpc_mutate]
pub async fn enqueue_dir(ctx: &Ctx, input: Input) -> Result<EnqueueDirResult, String> {
    use std::path::Path;

    let url = input
        .workflow
        .as_ref()
        .and_then(|w| w.url.as_deref())
        .ok_or_else(|| "enqueue_dir 需要 workflow.url (本地目录路径)".to_string())?;

    let dir = Path::new(url)
        .canonicalize()
        .map_err(|e| format!("目录不存在或无法访问 ({url}): {e}"))?;

    if !dir.is_dir() {
        return Err(format!("{url} 不是一个目录"));
    }

    let wf_root = config_rs::path::paths::workfolder();

    let DirScanOutput {
        inputs,
        result: res,
    } = scan_dir_videos(&dir, &wf_root, &input);
    for video_input in inputs {
        ctx.state.queue.enqueue(video_input);
    }
    tracing::info!(
        "[enqueue_dir] 扫描完成: scanned={} enqueued={} skipped={} errors={}",
        res.scanned,
        res.enqueued,
        res.skipped,
        res.errors
    );
    Ok(res)
}

/// 取消一个待执行任务 (仅 queued 状态可取消)。
#[fnrpc::rpc_mutate]
pub async fn cancel_queue(ctx: &Ctx, id: u64) -> Result<bool, String> {
    Ok(ctx.state.queue.cancel(id))
}
