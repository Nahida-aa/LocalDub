pub mod sf_ocr_dub;

use aa_workflow_core::Workflow;

use crate::input::Input;
use crate::workflows::args::{OutputMode, SubtitleSource};

/// 组合 → workflow id：`dub` × `sf_ocr` → `sf_ocr_dub`
/// （`{subtitle_source}_{output_mode}`）。
pub fn resolve_id(input: &Input) -> String {
    let source = SubtitleSource::from_input(input)
        .map(|s| s.as_str().to_string())
        .unwrap_or_else(|_| "asr".to_string());
    format!("{source}_{}", OutputMode::from_input(input).as_str())
}

/// id → 工作流定义。未注册的组合返回 `None`（调用方回退通用定义）。
pub fn build(workflow_id: &str) -> Option<Workflow> {
    match workflow_id {
        sf_ocr_dub::WORKFLOW_ID => Some(sf_ocr_dub::sf_ocr_dub().into_workflow()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input_with(output_mode: &str, source: &str) -> Input {
        serde_json::from_value(serde_json::json!({
            "workflow": {"output_mode": output_mode, "subtitleSource": source}
        }))
        .unwrap()
    }

    #[test]
    fn resolve_id_combines_source_and_output_mode() {
        assert_eq!(resolve_id(&input_with("dub", "sf_ocr")), "sf_ocr_dub");
        assert_eq!(resolve_id(&input_with("subtitle", "asr")), "asr_subtitle");
    }

    #[test]
    fn build_returns_sf_ocr_dub_and_none_for_unregistered() {
        let wf = build("sf_ocr_dub").expect("sf_ocr_dub 已注册");
        assert_eq!(wf.id, "sf_ocr_dub");
        assert!(build("asr_subtitle").is_none(), "未注册组合应回退 None");
    }
}
