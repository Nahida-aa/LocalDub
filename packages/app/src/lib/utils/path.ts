// media 走主服务器的 http /media (ServeDir)。
// 桌面 webview (webkitgtk) 的 <video>/<audio> 不支持自定义 scheme (asset://)
// 的流式播放, 视频/音频只能走 http —— 主服务器由桌面启动时自动拉起
// (src-tauri lib.rs), 保证可用; 浏览器 (手机/其它设备) 同样走此 URL。
export const mediaUrl = (path: string) => `http://localhost:19110/media/${path}`;
