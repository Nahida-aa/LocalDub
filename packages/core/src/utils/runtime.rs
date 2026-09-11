//! tokio 阻塞执行辅助 (CLI 等非 async 上下文)。

/// 在当前 tokio runtime 上 `block_on`; 若无运行中的 runtime 则自建 current_thread runtime。
///
/// 供 CLI / 同步代码在异步 API (mdns 发现等) 上取一时之需, 避免每个调用点各写一份。
pub fn block_on<T>(fut: impl std::future::Future<Output = T>) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(_) => tokio::runtime::Handle::current().block_on(fut),
        Err(_) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(fut)
        }
    }
}
