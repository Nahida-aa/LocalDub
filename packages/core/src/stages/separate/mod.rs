//! separate: demucs 分离人声与背景声, 提升 tts-vc 的质量。
//!
//! 镜像 TS `packages/core/stages/separate/`。目前是占位状态:
//! - 数据结构已落地 ([`args`])
//! - 主逻辑 `stage_separate` 尚未移植 (返回明确错误)

pub mod args;

use crate::context::TaskCtx;

pub use args::SeparateArgs;

/// 从 `ctx.input.stages.separate` 解析配置 (与 TS default 对齐)
pub fn read_args(ctx: &TaskCtx) -> SeparateArgs {
    ctx.input
        .get("stages")
        .and_then(|v| v.get("separate"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// 占位入口 (镜像 TS `stageSeparate`)
///
/// TODO: 尚未移植。待迁移: demucs 分离 / stems 输出 / setStage 完成标记。
pub fn stage_separate(ctx: &TaskCtx) -> Result<(), String> {
    let cfg = read_args(ctx);
    Err(format!(
        "separate 尚未移植 (runtime={:?}, device={:?}, always={})",
        cfg.runtime, cfg.device, cfg.always
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx_with_input(input: serde_json::Value) -> TaskCtx {
        crate::context::read_ctx_from_value(input).unwrap()
    }

    #[test]
    fn field_defaults_when_absent() {
        let ctx = ctx_with_input(json!({
            "task": {"id": "t", "task_dir": "/x", "url": "http://e", "source": "remote",
                     "status": "running", "created_at": "2024-01-01T00:00:00Z"},
            "input": {"stages": {"separate": {}}}
        }));
        let cfg = read_args(&ctx);
        assert_eq!(cfg.runtime, args::Runtime::BurnTch);
        assert_eq!(cfg.device, args::Device::Cuda);
        assert!(!cfg.always);
        assert!(cfg.stems.is_empty());
    }

    #[test]
    fn read_args_parses_camel_case_fields() {
        let ctx = ctx_with_input(json!({
            "task": {"id": "t", "task_dir": "/x", "url": "http://e", "source": "remote",
                     "status": "running", "created_at": "2024-01-01T00:00:00Z"},
            "input": {"stages": {"separate": {
                "runtime": "burn-tch",
                "device": "mps",
                "always": true,
                "stems": ["drums", "vocals"]
            }}}
        }));
        let cfg = read_args(&ctx);
        assert_eq!(cfg.device, args::Device::Mps);
        assert!(cfg.always);
        assert_eq!(cfg.stems, vec![args::Stem::Drums, args::Stem::Vocals]);
    }
}
