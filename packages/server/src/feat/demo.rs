#[fnrpc::rpc_subscribe]
pub fn tick(interval_ms: u64) -> impl futures::Stream<Item = u64> {
    futures::stream::unfold(0u64, move |count| async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;
        println!("tick: {count}");
        Some((count, count + 1))
    })
}
