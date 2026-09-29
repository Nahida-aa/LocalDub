pub mod sf_ocr_dub;

use aa_workflow_core::Workflow;

use crate::state::State;

/// 组合 → workflow id：`dub` × `sf_ocr` → `sf_ocr_dub`
/// （`{subtitle_source}_{output_mode}`）。
pub fn resolve_id(ctx: &State) -> String {
    let source = ctx
        .input
        .workflow
        .as_ref()
        .map(|w| w.subtitle_source.as_str())
        .unwrap_or("asr");
    format!("{source}_{}", ctx.init.output_mode)
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
    use crate::state::read_ctx_from_value;
    use serde_json::json;

    fn ctx(output_mode: &str, source: &str) -> State {
        read_ctx_from_value(json!({
            "workflow": {"id":"t","video_dir":"/t","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
            "input": {"workflow": {"subtitleSource": source}},
            "output_mode": output_mode
        }))
        .unwrap()
    }

    #[test]
    fn resolve_id_combines_source_and_output_mode() {
        assert_eq!(resolve_id(&ctx("dub", "sf_ocr")), "sf_ocr_dub");
        assert_eq!(resolve_id(&ctx("subtitle", "asr")), "asr_subtitle");
    }

    #[test]
    fn build_returns_sf_ocr_dub_and_none_for_unregistered() {
        let wf = build("sf_ocr_dub").expect("sf_ocr_dub 已注册");
        assert_eq!(wf.id, "sf_ocr_dub");
        assert!(build("asr_subtitle").is_none(), "未注册组合应回退 None");
    }
}
