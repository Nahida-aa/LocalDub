// aa-workflow 引擎集成——**开发中，整体暂停编译**（2026-09-30）。
//
// 为后续「声明式 DAG（数据结构即 DAG）/ 命令式 DAG（代码即 DAG）」改造让路，
// 引擎路径暂时下线：
// - `ExecPath::Engine` 分支已改为打印「开发中」（cmd/workflows/{start,continue_workflow}.rs）；
// - 依赖本模块的测试（continue_workflow.rs 的引擎接线测试）一并注释。
//
// 恢复时：取消下面各行注释，并把上述分支/测试还原。
//
// pub mod engine;
// pub mod workflows;
// pub mod engine_store;
// #[cfg(test)]
// pub(crate) mod test_support;
// pub use engine::{EngineOptions, run_workflow_engine};
