import { mediaUrl } from "#/lib/utils/path.ts";
import { useActivePath, useMediaVersion } from "../store/ContentPanel";

export interface VideoPanelProps {
  path: string;
  onReady: (ref: HTMLVideoElement) => void;
}

export function VideoPanel(props: VideoPanelProps) {
  let videoRef!: HTMLVideoElement;
  // 文件树事件命中本媒体路径时版本号自增，拼到 ?v= 强制浏览器重新拉取（避开
  // 流式写入中途的脏缓存）。二进制走 axum ServeDir，不进 TanStack Query。
  const version = useMediaVersion(props.path);
  const src = () => `${mediaUrl(props.path)}?v=${version()}`;
  return (
    <div class="flex items-center justify-center bg-black h-full w-full overflow-hidden">
      <video
        ref={videoRef}
        src={src()}
        // controls
        class="max-h-full max-w-full object-contain"
        onLoadedMetadata={() => props.onReady(videoRef)}
      />
    </div>
  );
}
