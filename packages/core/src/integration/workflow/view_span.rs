//! tracing 中间件：在引擎执行（spawn 的 task）内打开带 video_dir 的 workflow span。
//!
//! 为什么需要 middleware 而不是在 LocalDub 调用侧包 span：
//! `run_workflow` 内部 `tokio::spawn` 一个 task 跑引擎，tokio 新 task **不继承**
//! 调用方的 span。LocalDub 在 `drive()` 里 `.instrument(span)` 只覆盖「等待流」的
//! 一侧，真正产生 step 日志的引擎 task 不在 span 内，`WorkflowFileLayer` 拿不到
//! video_dir → 不落盘 `<video_dir>/<id>.log`。
//!
//! middleware 由引擎在 spawn 的 task 内调用，从 `ctx.input` 取 video_dir 开 span
//! 包住 handler —— 于是 handler 内所有 step 的 tracing 事件都带 video_dir。

use aa_workflow_core::middleware::{Middleware, MiddlewareServerFnArgs};
use tracing::Instrument;

/// 注册到 workflow 定义：包住 handler，打开 `workflow{video_dir=...}` span。
pub fn view_span_middleware() -> Middleware {
    Middleware::new().server(|mut args: MiddlewareServerFnArgs| async move {
        let video_dir = args
            .ctx
            .input
            .get("workflow")
            .and_then(|w| w.get("video_dir"))
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string();
        // 诊断：确认 middleware 在引擎 task 内执行且拿到 video_dir。
        tracing::info!(target: "env", "view_span middleware: video_dir={video_dir}");
        // 注意用裸值（不用 % / ?）：% 走 record_debug，SpanFields 的 record_debug
        // 用 {:?} 会给路径加上引号 → 拼出的 log 路径带引号 → 写不进任何文件。
        // 旧 pipeline 的 `video_dir = video_dir` 走 record_str，是干净的字符串。
        let span = tracing::info_span!("workflow", video_dir = video_dir.as_str());
        args.next(None).instrument(span).await
    })
}

#[cfg(test)]
mod tests_without_middleware {
    use aa_workflow_core::{BaseCtx, CreateWorkflowConfig, InMemoryStore, RunWorkflowOptions, WorkflowEvent, create_workflow, run_workflow};
    use std::sync::Arc;
    use tracing::Instrument;

    /// 对照实验：不用 middleware，只在调用侧包 span。
    /// 验证 tokio::spawn 是否继承调用方 span（我之前的判断是"不继承"）。
    #[tokio::test]
    async fn caller_side_span_without_middleware() {
        let _ = crate::logging::init();
        let tmp = std::env::temp_dir().join(format!("caller_span_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let vd = tmp.to_string_lossy().into_owned();

        // 注意：不注册 view_span_middleware
        let wf = create_workflow(CreateWorkflowConfig::new("caller-span-test").input::<serde_json::Value>())
            .handler(|ctx: BaseCtx<serde_json::Value, serde_json::Value>| async move {
                tracing::info!(target: "test", "CALLER SPAN MARKER");
                let _ = ctx;
                Ok(serde_json::Value::Null)
            });

        let input = serde_json::json!({"workflow": {"video_dir": vd}});
        let events = async {
            run_workflow(
                RunWorkflowOptions::new(Arc::new(wf), Arc::new(InMemoryStore::new()))
                    .input(input),
            )
            .collect()
            .await
        }
        .instrument(tracing::info_span!("workflow", video_dir = vd.as_str()))
        .await;
        assert!(events.iter().any(|e| matches!(e, WorkflowEvent::RunFinished { .. })));
        let log = tmp.join(format!("{}.log", tmp.file_name().unwrap().to_str().unwrap()));
        match std::fs::read_to_string(&log) {
            Ok(content) => eprintln!("CALLER-SIDE LOG OK:\n{}", content),
            Err(e) => eprintln!("CALLER-SIDE LOG MISSING: {e}  ← tokio::spawn 不继承调用方 span"),
        }
    }
}

#[cfg(test)]
mod tests_with_real_dag_policies {
    use aa_workflow_core::{BaseCtx, CreateWorkflowConfig, InMemoryStore, RunWorkflowOptions, WorkflowEvent, create_workflow, run_workflow};
    use aa_workflow_middlewares::{DagSpec, dag_policies};
    use std::sync::Arc;

    /// 用真实的 dag_policies（空图）复现真实 middleware 链，验证 span 是否还落盘。
    #[tokio::test]
    async fn real_dag_policies_keeps_span() {
        let _ = crate::logging::init();
        let tmp = std::env::temp_dir().join(format!("dag_policies_span_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let vd = tmp.to_string_lossy().into_owned();

        let spec = DagSpec::<serde_json::Value, serde_json::Value>::new("empty");
        let shape = Arc::new(spec.shape().unwrap());

        let wf = create_workflow(CreateWorkflowConfig::new("dag-policies-test").input::<serde_json::Value>())
            .middleware(super::view_span_middleware())
            .middleware(dag_policies(shape))
            .handler(|ctx: BaseCtx<serde_json::Value, serde_json::Value>| async move {
                tracing::info!(target: "test", "DAG POLICIES MARKER");
                let _ = ctx;
                Ok(serde_json::Value::Null)
            });

        let input = serde_json::json!({"workflow": {"video_dir": vd}});
        let events = run_workflow(
            RunWorkflowOptions::new(Arc::new(wf), Arc::new(InMemoryStore::new()))
                .input(input),
        )
        .collect()
        .await;
        assert!(events.iter().any(|e| matches!(e, WorkflowEvent::RunFinished { .. })));
        let log = tmp.join(format!("{}.log", tmp.file_name().unwrap().to_str().unwrap()));
        match std::fs::read_to_string(&log) {
            Ok(content) => eprintln!("DAG POLICIES LOG OK:\n{}", content),
            Err(e) => panic!("DAG POLICIES LOG MISSING: {e}"),
        }
    }
}

#[cfg(test)]
mod tests_with_dag_handler {
    use aa_workflow_core::{BaseCtx, CreateWorkflowConfig, InMemoryStore, RunWorkflowOptions, WorkflowEvent, create_workflow, run_workflow};
    use aa_workflow_middlewares::{DagSpec, dag_to_handler, dag_policies};
    use std::sync::Arc;

    /// 用真实 dag_to_handler + 单节点 spec（节点闭包内打日志），复现真实 handler 路径。
    #[tokio::test]
    async fn dag_handler_keeps_span() {
        let _ = crate::logging::init();
        let tmp = std::env::temp_dir().join(format!("dag_handler_span_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let vd = tmp.to_string_lossy().into_owned();

        let spec = DagSpec::<serde_json::Value, serde_json::Value>::new("one-node")
            .node("step1", Vec::<String>::new(), |_sc, _deps| async move {
                tracing::info!(target: "test", "DAG NODE MARKER");
                Ok(serde_json::Value::Null)
            });
        let spec = Arc::new(spec);
        let shape = Arc::new(spec.shape().unwrap());

        let wf = create_workflow(CreateWorkflowConfig::new("dag-handler-test").input::<serde_json::Value>())
            .middleware(super::view_span_middleware())
            .middleware(dag_policies(shape))
            .handler(dag_to_handler(spec));

        let input = serde_json::json!({"workflow": {"video_dir": vd}});
        let events = run_workflow(
            RunWorkflowOptions::new(Arc::new(wf), Arc::new(InMemoryStore::new()))
                .input(input),
        )
        .collect()
        .await;
        assert!(events.iter().any(|e| matches!(e, WorkflowEvent::RunFinished { .. })), "run 应完成: {events:?}");
        let log = tmp.join(format!("{}.log", tmp.file_name().unwrap().to_str().unwrap()));
        match std::fs::read_to_string(&log) {
            Ok(content) => eprintln!("DAG HANDLER LOG OK:\n{}", content),
            Err(e) => panic!("DAG HANDLER LOG MISSING: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aa_workflow_core::{BaseCtx, CreateWorkflowConfig, InMemoryStore, RunWorkflowOptions, WorkflowEvent, create_workflow, run_workflow};
    use std::sync::Arc;

    #[tokio::test]
    async fn span_reaches_file_layer() {
        let _ = crate::logging::init();
        // 模拟真实路径：含中文、含 "./" 段
        let tmp = std::env::temp_dir().join(format!("view_span测试_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let vd = tmp.to_string_lossy().into_owned();

        let wf = create_workflow(CreateWorkflowConfig::new("span-test").input::<serde_json::Value>())
            .middleware(view_span_middleware())
            .middleware(Middleware::new())
            .handler(|ctx: BaseCtx<serde_json::Value, serde_json::Value>| async move {
                tracing::info!(target: "test", "STEP LOG MARKER");
                let ev = aa_workflow_core::WorkflowEvent::RunStarted {
                    ts: 0,
                    run_id: "test".into(),
                    thread_id: None,
                };
                tracing::info!(target: "test", message = ?ev);
                let _ = ctx;
                Ok(serde_json::Value::Null)
            });

        let input = serde_json::json!({"workflow": {"video_dir": vd}});
        let events = run_workflow(
            RunWorkflowOptions::new(Arc::new(wf), Arc::new(InMemoryStore::new()))
                .input(input),
        )
        .collect()
        .await;
        assert!(events.iter().any(|e| matches!(e, WorkflowEvent::RunFinished { .. })));
        let log = tmp.join(format!("{}.log", tmp.file_name().unwrap().to_str().unwrap()));
        match std::fs::read_to_string(&log) {
            Ok(content) => {
                eprintln!("LOG CONTENT:\n{}", content);
                assert!(content.contains("STEP LOG MARKER"), "step 日志应落盘");
            }
            Err(e) => panic!("log 文件不存在 ({:?}): {e}", log),
        }
    }
}
