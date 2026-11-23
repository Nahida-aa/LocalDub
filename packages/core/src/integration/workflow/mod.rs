// aa-workflow 引擎集成。
//
// 只保留 [`engine_store`]：FsRunStore 是 async RunStore 的文件实现，
// 将来接入 DAG 中间件方案时直接复用。其余过渡代码（engine / workflows /
// test_support）已删除——原版 / parallel / engine 三条过渡路线互不混用。
pub mod engine_store;
