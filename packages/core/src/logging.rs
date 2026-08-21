//! 日志基础设施: 任务级文件落盘 Layer + 统一初始化。
//!
//! 取代旧的手搓 `emit_log` + thread_local `LOG_CTX` 方案:
//! - 上下文用 tracing span: pipeline 入口进入 `task` span (携带 `task_dir` 字段),
//!   stage 入口进入 `stage` span (携带 `stage` 字段)。
//! - 落盘用 [`TaskFileLayer`]: 订阅事件, 从当前 span 栈提取 `task_dir` 写到
//!   `<task_dir>/<tid>.log`, 行格式与重构前一致 (`[时间] [stage] 文本`)。
//! - 级别由调用处 `tracing::{info,warn,error}!` 决定, 不再依赖消息文本前缀。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use crate::stages::utils::{now_iso, task_id};

/// 存在 span extensions 里的字段值, 供 on_event 读取。
#[derive(Default, Clone)]
struct SpanFields {
    task_dir: Option<String>,
    stage: Option<String>,
}

impl Visit for SpanFields {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "task_dir" => self.task_dir = Some(value.to_string()),
            "stage" => self.stage = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "task_dir" => self.task_dir = Some(format!("{value:?}")),
            "stage" => self.stage = Some(format!("{value:?}")),
            _ => {}
        }
    }
}

/// 自定义 Layer: 把事件追加写到当前 task 的 `<task_dir>/<tid>.log`。
///
/// 不依赖 thread_local: 在 `on_new_span` 时把 `task_dir`/`stage` 字段值存进 span 的
/// extensions, `on_event` 时从事件所在 span 作用域提取并落盘。无 `task` span 时不写文件。
struct TaskFileLayer;

impl TaskFileLayer {
    /// 从事件所在的 span 作用域提取 `task_dir` / `stage`。
    fn extract<'a, S>(ctx: &Context<'a, S>, event: &Event<'_>) -> SpanFields
    where
        S: Subscriber + for<'b> LookupSpan<'b>,
    {
        let mut out = SpanFields::default();
        // event_scope 从事件的父 span 到根遍历; 后者覆盖前者。
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope {
                if let Some(f) = span.extensions().get::<SpanFields>() {
                    if f.task_dir.is_some() {
                        out.task_dir = f.task_dir.clone();
                    }
                    if f.stage.is_some() {
                        out.stage = f.stage.clone();
                    }
                }
            }
        }
        out
    }
}

impl<S> Layer<S> for TaskFileLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        let mut f = SpanFields::default();
        attrs.record(&mut f);
        if f.task_dir.is_none() && f.stage.is_none() {
            return;
        }
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(f);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let fields = Self::extract(&ctx, event);
        let Some(task_dir) = fields.task_dir else {
            return; // 不在 task 上下文, 不落盘
        };
        let Some(tid) = task_id(&task_dir) else {
            return;
        };
        let log_path = Path::new(&task_dir).join(format!("{tid}.log"));
        let stage_prefix = fields
            .stage
            .map(|s| format!("[{s}] "))
            .unwrap_or_default();

        // 取事件的 message 字段作为文本主体。
        let mut msg = String::new();
        event.record(&mut MsgVisitor { out: &mut msg });
        if msg.is_empty() {
            return;
        }

        let entry = format!("[{}] {stage_prefix}{msg}\n", now_iso());
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let _ = f.write_all(entry.as_bytes());
        }
    }
}

/// 提取事件的 message 字段文本。
struct MsgVisitor<'a> {
    out: &'a mut String,
}

impl<'a> Visit for MsgVisitor<'a> {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            *self.out = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            *self.out = format!("{value:?}");
        }
    }
}

/// 统一初始化 tracing: fmt (stderr) + 任务文件落盘 + EnvFilter。
///
/// 二进制入口 (`cli`/`server`/`app`) 调用一次即可, 取代各自内联的
/// `tracing_subscriber::fmt()...init()`。重复调用会失败 (Subscriber 只能初始化一次),
/// 故返回 `anyhow::Result` 由调用方按需忽略。
pub fn init() -> anyhow::Result<()> {
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(true),
        )
        .with(TaskFileLayer)
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init()
        .map_err(|e| anyhow::anyhow!("tracing 初始化失败: {e}"))
}
