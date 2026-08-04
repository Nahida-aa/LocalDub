import { For, createSignal } from "solid-js";
import { useScrollSync } from "#/hooks/useScrollSync";
import { TimelineToolbar } from "./TimelineToolbar";
import { TimelineRuler } from "./TimelineRuler";
import { BASE_PX_PER_MS, rulerConfig } from "./consts";
import {
  CommonTimelineTracks,
  type CommonTrackGroup,
  getCommonTimelineSidebarLabels,
} from "./CommonTimelineTracks";
import type { FrameRate } from "@repo/core/utils/timecode";

interface Props {
  groups: CommonTrackGroup[];
  duration: number;
  currentTime: number;
  fps: FrameRate;
  onSeek: (ms: number) => void;
  taskDir?: string;
}

export function CommonTimeline(props: Props) {
  const fpsFloat = () => props.fps.numerator / props.fps.denominator;

  const ZOOM_MIN = 0.1;
  const ZOOM_MAX = 100;
  const [sliderPos, setSliderPos] = createSignal(0.38);
  const zoom = () => ZOOM_MIN * Math.pow(ZOOM_MAX / ZOOM_MIN, sliderPos());

  const pxPerMs = () => BASE_PX_PER_MS * zoom();
  const totalPx = () => props.duration * pxPerMs();
  const rc = () => rulerConfig(pxPerMs(), props.fps);

  let tracksRef!: HTMLDivElement;
  let rulerRef!: HTMLDivElement;
  let labelsRef!: HTMLDivElement;

  const [scrollLeft, setScrollLeft] = createSignal(0);

  const handleTrackScroll = () => {
    setScrollLeft(tracksRef?.scrollLeft ?? 0);
  };

  useScrollSync(
    () => tracksRef,
    () => rulerRef,
    () => labelsRef,
  );

  const playheadLeft = () => props.currentTime * pxPerMs() - scrollLeft();

  const onSliderChange = (v: number) => {
    const oldZoom = zoom();
    setSliderPos(v);
    const newZoom = zoom();
    const playheadPx = props.currentTime * BASE_PX_PER_MS * oldZoom - scrollLeft();
    const newScrollLeft = Math.max(0, props.currentTime * BASE_PX_PER_MS * newZoom - playheadPx);
    requestAnimationFrame(() => {
      if (tracksRef) tracksRef.scrollLeft = newScrollLeft;
    });
  };

  let rightRef!: HTMLDivElement;
  let playheadDragging = false;

  const onPlayheadDown = (e: PointerEvent) => {
    e.preventDefault();
    playheadDragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  };

  const onPlayheadMove = (e: PointerEvent) => {
    if (!playheadDragging) return;
    const rect = rightRef.getBoundingClientRect();
    const x = e.clientX - rect.left + scrollLeft();
    const ms = x / pxPerMs();
    props.onSeek(Math.max(0, Math.min(ms, props.duration)));
  };

  const onPlayheadUp = () => {
    playheadDragging = false;
  };

  // 侧边栏标签（扁平化，每行一个，带颜色）
  const sidebarLabels = () =>
    getCommonTimelineSidebarLabels(props.groups);

  return (
    <div class="flex flex-col h-full border-t select-none">
      <TimelineToolbar zoom={zoom()} sliderValue={sliderPos()} onSliderChange={onSliderChange} />

      <div class="flex flex-1 overflow-hidden">
        {/* 侧边栏 */}
        <div class="w-30 shrink-0 border-r flex flex-col">
          <div class="border-b bg-muted/20 shrink-0">
            <div class="h-5" />
          </div>
          <div ref={labelsRef!} class="flex-1 overflow-hidden">
            <For each={sidebarLabels()}>
              {(lbl) => (
                <div
                  class="h-16 border-b flex items-center px-3 text-xs text-muted-foreground truncate"
                  style={{ "border-left": `3px solid ${lbl.color ?? "#888"}` }}
                >
                  {lbl.label}
                </div>
              )}
            </For>
          </div>
        </div>

        {/* 内容区 */}
        <div ref={rightRef!} class="flex-1 flex flex-col min-w-0 relative overflow-hidden">
          <TimelineRuler
            ref={(el) => rulerRef = el}
            totalPx={totalPx()}
            duration={props.duration}
            rulerCfg={rc()}
            pxPerMs={pxPerMs()}
            fps={fpsFloat()}
            onSeek={props.onSeek}
          />

          <CommonTimelineTracks
            ref={(el) => tracksRef = el}
            groups={props.groups}
            totalPx={totalPx()}
            pxPerMs={pxPerMs()}
            onSeek={props.onSeek}
            onScroll={handleTrackScroll}
            taskDir={props.taskDir}
          />

          {/* Playhead */}
          <div
            class="absolute top-0 h-full w-0.5 bg-red-500/50 z-10 pointer-events-none"
            style={{ left: `${playheadLeft()}px` }}
          >
            <div
              class="absolute -top-1.5 -left-1.5 w-3 h-3 rounded-full bg-red-500 border-2 border-white shadow-sm cursor-pointer pointer-events-auto"
              onPointerDown={onPlayheadDown}
              onPointerMove={onPlayheadMove}
              onPointerUp={onPlayheadUp}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
