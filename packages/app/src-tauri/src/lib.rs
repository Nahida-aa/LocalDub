// 桌面端: 纯 UI 宿主。
//
// UI 全部走 http (fnrpc + media) 打主服务器 (127.0.0.1:19110), 与浏览器同构;
// 主服务器由下方启动时自动拉起 (独立进程, 幂等), 生命周期不绑定本窗口。

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 统一初始化 tracing (fmt + 任务文件落盘 + EnvFilter)。
    // 重复 init 会失败, 故忽略返回值 (server 内部若已初始化则跳过)。
    let _ = ld_core::logging::init();

    // 启动时拉起主服务器 (幂等, 已在运行则跳过): 视频播放依赖 http /media
    // (webkitgtk 的 <video> 仅支持 http 流式), 默认保证桌面开箱即用。
    // 失败仅记日志, UI 的 Main Server 卡片可手动重试 (fnrpc start_main)。
    tauri::async_runtime::spawn_blocking(move || {
        match ld_core::cmd::servers::start_main_server(false) {
            Ok(msg) => tracing::info!("[main] {msg}"),
            Err(e) => tracing::warn!("[main] 主服务器自动启动失败: {e:#}"),
        }
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
