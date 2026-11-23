// aa-workflow 引擎集成。
//
// - [`engine_store`]：FsRunStore 是 async RunStore 的文件实现；
// - [`engine`]：start 路径的 core 直驱（run_workflow + FsRunStore）。
// continue / resume 尚未接入（留 TODO，见 docs/workflow/INTEGRATION-PLAN.md）。
// 原版 / parallel / engine 三条过渡路线互不混用。
pub mod engine;
pub mod engine_store;
