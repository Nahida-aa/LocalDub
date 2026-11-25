//! 日志基础设施: 任务级文件落盘 Layer + 统一初始化。
//!
//! 取代旧的手搓 `emit_log` + thread_local `LOG_CTX` 方案:
//! - 上下文用 tracing span: pipeline 入口进入 `workflow` span (携带 `video_dir` 字段),
//!   step 入口进入 `step` span (携带 `step` 字段)。
//! - 落盘用 [`WorkflowFileLayer`]: 订阅事件, 从当前 span 栈提取 `video_dir` 写到
//!   `<video_dir>/<tid>.log`, 行格式与重构前一致 (`[时间] [step] 文本`)。
//! - 级别由调用处 `tracing::{info,warn,error}!` 决定, 不再依赖消息文本前缀。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use crate::steps::utils::{now_iso, video_id};

/// 存在 span extensions 里的字段值, 供 on_event 读取。
#[derive(Default, Clone)]
struct SpanFields {
    video_dir: Option<String>,
    step: Option<String>,
}

impl Visit for SpanFields {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "video_dir" => self.video_dir = Some(value.to_string()),
            "step" => self.step = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        // % / ? 修饰的字段会走这里，{:?} 会给字符串加引号——去掉首尾引号，
        // 免得拼出的 log 路径带引号写不进文件。
        let raw = format!("{value:?}");
        let unquoted = raw
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(&raw)
            .to_string();
        match field.name() {
            "video_dir" => self.video_dir = Some(unquoted),
            "step" => self.step = Some(unquoted),
            _ => {}
        }
    }
}

/// 自定义 Layer: 把事件追加写到当前 workflow 的 `<video_dir>/<tid>.log`。
///
/// 不依赖 thread_local: 在 `on_new_span` 时把 `video_dir`/`step` 字段值存进 span 的
/// extensions, `on_event` 时从事件所在 span 作用域提取并落盘。无 `workflow` span 时不写文件。
struct WorkflowFileLayer;

impl WorkflowFileLayer {
    /// 从事件所在的 span 作用域提取 `video_dir` / `step`。
    fn extract<'a, S>(ctx: &Context<'a, S>, event: &Event<'_>) -> SpanFields
    where
        S: Subscriber + for<'b> LookupSpan<'b>,
    {
        let mut out = SpanFields::default();
        // event_scope 从事件的父 span 到根遍历; 后者覆盖前者。
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope {
                if let Some(f) = span.extensions().get::<SpanFields>() {
                    if f.video_dir.is_some() {
                        out.video_dir = f.video_dir.clone();
                    }
                    if f.step.is_some() {
                        out.step = f.step.clone();
                    }
                }
            }
        }
        out
    }
}

impl<S> Layer<S> for WorkflowFileLayer
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
        if f.video_dir.is_none() && f.step.is_none() {
            return;
        }
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(f);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let fields = Self::extract(&ctx, event);
        let Some(video_dir) = fields.video_dir else {
            return; // 不在 workflow 上下文, 不落盘
        };
        let Some(tid) = video_id(&video_dir) else {
            return;
        };
        let log_path = Path::new(&video_dir).join(format!("{tid}.log"));
        let step_prefix = fields.step.map(|s| format!("[{s}] ")).unwrap_or_default();

        // 取事件的 message 字段作为文本主体。
        let mut msg = String::new();
        event.record(&mut MsgVisitor { out: &mut msg });
        if msg.is_empty() {
            return;
        }

        let level = event.metadata().level();
        let entry = format!("[{}] [{level}] {step_prefix}{msg}\n", now_iso());
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&log_path) {
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

/// 终端 event formatter：span 只显示 video_dir 的 basename（如 `90:`），
/// 不显示 `workflow{video_dir=完整路径}` 这层壳。其它（时间戳/等级/target/message）照旧。
struct ShortSpanFormat;

impl<S, N> tracing_subscriber::fmt::format::FormatEvent<S, N> for ShortSpanFormat
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::format::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        use std::fmt::Write as _;
        use tracing_subscriber::fmt::format::FormatFields as _;
        let meta = event.metadata();
        write!(writer, "{} ", crate::steps::utils::now_iso())?;
        write!(writer, "{} ", meta.level())?;

        // span 部分：只取 video_dir 字段的 basename，输出 `90:`。
        if let Some(scope) = ctx.event_scope() {
            for span in scope.from_root() {
                let ext = span.extensions();
                if let Some(fields) = ext.get::<tracing_subscriber::fmt::FormattedFields<N>>() {
                    // 字段形如 `video_dir=/path/90`（自定义 fmt_fields 已截成 basename）。
                    for part in fields.as_str().split_whitespace() {
                        if let Some(val) = part.strip_prefix("video_dir=") {
                            write!(writer, "{val}: ")?;
                        }
                    }
                }
            }
        }

        write!(writer, "{}: ", meta.target())?;
        ctx.format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

/// 统一初始化 tracing: fmt (stderr) + 任务文件落盘 + EnvFilter。
///
/// 二进制入口 (`cli`/`server`/`app`) 调用一次即可, 取代各自内联的
/// `tracing_subscriber::fmt()...init()`。重复调用会失败 (Subscriber 只能初始化一次),
/// 故返回 `anyhow::Result` 由调用方按需忽略。
pub fn init() -> anyhow::Result<()> {
    use tracing_subscriber::prelude::*;
    // 终端日志：video_dir 字段只显示 basename（如 82），不显示完整路径——
    // 完整路径在文件日志里用于定位 <id>.log，终端看见视频号就够。
    let short_fields = tracing_subscriber::fmt::format::debug_fn(
        |writer: &mut tracing_subscriber::fmt::format::Writer<'_>,
         field: &tracing::field::Field,
         value: &dyn std::fmt::Debug|
         -> std::fmt::Result {
            if field.name() == "message" {
                // message 是日志正文：默认格式显示纯文本，不带 `message=` 前缀和引号。
                let raw = format!("{value:?}");
                write!(writer, "{}", raw.trim_matches('"'))
            } else if field.name() == "video_dir" {
                let raw = format!("{value:?}");
                let unquoted = raw.trim_matches('"');
                let base = std::path::Path::new(unquoted)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(unquoted);
                write!(writer, "{}={}", field, base)
            } else {
                write!(writer, "{}={:?}", field, value)
            }
        },
    );
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(true)
                .fmt_fields(short_fields)
                .event_format(ShortSpanFormat),
        )
        .with(WorkflowFileLayer)
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init()
        .map_err(|e| anyhow::anyhow!("tracing 初始化失败: {e}"))
}
