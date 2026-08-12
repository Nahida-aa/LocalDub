//! 用 specta + specta-jsonschema 生成 input 的 JSON Schema。
//!
//! 对标 TS 侧 `packages/cli/scripts/gen-input-schema.ts`（zod `toJSONSchema({io:'input'})`）。
//!
//! 类型定义已迁到 [`core_rs::input`]，本 bin 只负责导出：
//! `PhasesFormat::Deserialize` 面 = input schema (root `Input_Deserialize`)。
//! 运行时由 `#[serde(default)]` 补齐默认值。specta 无 `Ranged`/min/max 约束，
//! 数值范围需在解析时自行校验（见 input 模块 TODO）。

use std::borrow::Cow;

use core_rs::input::Input;
use specta::datatype::{DataType, Reference};
use specta::{Format as _, Type, Types};
use specta_jsonschema::JsonSchema;
use specta_serde::{Phase, PhasesFormat, select_phase_datatype};

/// 占位 formatter：types 已由 [`PhasesFormat`] 预映射，导出时不再二次改写。
struct NoopFormat;

impl specta::Format for NoopFormat {
    fn map_types(&self, types: &Types) -> Result<Cow<'_, Types>, specta::FormatError> {
        Ok(Cow::Owned(types.clone()))
    }

    fn map_type(
        &self,
        _types: &Types,
        dt: &DataType,
    ) -> Result<Cow<'_, DataType>, specta::FormatError> {
        Ok(Cow::Owned(dt.clone()))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut types = Types::default();
    let root = Input::definition(&mut types);
    let phased = PhasesFormat
        .map_types(&types)
        .map_err(|e| format!("phases: {e}"))?
        .into_owned();
    let deser = select_phase_datatype(&root, &phased, Phase::Deserialize);
    let name = match &deser {
        DataType::Reference(Reference::Named(r)) => phased
            .get(r)
            .map(|ndt| ndt.name.clone())
            .ok_or_else(|| "deserialize root ref not in phased types".to_string())?,
        other => return Err(format!("unexpected deserialize root: {other:?}").into()),
    };
    let out = config_rs::root::repo_root().join("input.schema.json");
    let schema = JsonSchema::default()
        .allow_additional_properties(true)
        .title("LocalDub 输入")
        .export_ref_value(&phased, NoopFormat, &name)?;
    std::fs::write(&out, serde_json::to_string_pretty(&schema).unwrap())?;
    println!("Generated: {} (root {name})", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_marks_defaulted_fields_optional() {
        let mut types = Types::default();
        let root = Input::definition(&mut types);
        let phased = PhasesFormat
            .map_types(&types)
            .map_err(|e| format!("phases: {e}"))
            .unwrap()
            .into_owned();
        let deser = select_phase_datatype(&root, &phased, Phase::Deserialize);
        let DataType::Reference(Reference::Named(r)) = &deser else {
            panic!("expected named deserialize root");
        };
        let ndt = phased.get(r).unwrap();
        let DataType::Struct(strct) = &ndt.ty.as_ref().unwrap() else {
            panic!("expected struct root");
        };
        let fields = match &strct.fields {
            specta::datatype::Fields::Named(f) => &f.fields,
            _ => panic!("expected named fields"),
        };
        let optional = fields
            .iter()
            .filter(|(_, f)| f.optional)
            .map(|(n, _)| n.as_ref())
            .collect::<Vec<_>>();
        assert!(optional.contains(&"task"), "task 应可选: {optional:?}");
        assert!(
            optional.contains(&"command"),
            "command 应可选: {optional:?}"
        );
    }
}
