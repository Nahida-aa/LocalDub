// aa-workflow 引擎集成。
//
// 2026-11-23 恢复编译：aa-workflow 已升到 async RunStore rev（7b2d3fc），
// FsRunStore 已改为 async + 单写者 actor + per-run 目录。
pub mod engine;
pub mod engine_store;
#[cfg(test)]
pub(crate) mod test_support;
pub use engine::{EngineOptions, run_workflow_engine};
