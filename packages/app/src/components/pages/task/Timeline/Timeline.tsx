import { createSignal, onMount, onCleanup } from "solid-js";
import { useScrollSync } from "#/hooks/useScrollSync";
import { TimelineToolbar } from "./TimelineToolbar";
import { TimelineRuler } from "./TimelineRuler";
import { TimelineTrackSide } from "./TimelineTrackSide";
import { TimelineTracks } from "./TimelineTracks";
import { BASE_PX_PER_MS, rulerConfig, trackColor } from "./consts";
export type { Track, TrackSegment } from "./consts";
import type { Track } from "./consts";
import type { FrameRate } from "@repo/core/utils/timecode";
import { trace } from "#/lib/debugLog.ts";

interface Props {
  tracks: Track[];
  duration: number;
  currentTime: number;
  fps: FrameRate;
  onSeek: (ms: number) => void;
  taskDir?: string;
}

let _timelineMount = 0;

export function Timeline(props: Props) {
  const myMount = ++_timelineMount;
  trace(`[EL-MOUNT] Timeline #${myMount} taskDir=${props.taskDir}`);
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

  onMount(() => {
    const container = tracksRef;
    (container as any).__pid = myMount;
    // 测试：禁用 overflow-anchor，看是否阻止滚动被浏览器内部重置
    (container as any).style.overflowAnchor = "none";

    // 同步抓 scrollLeft 变化的瞬间（scroll 事件）
    const lastEvtSl = { v: container.scrollLeft };
    const onSc = () => {
      const v = container.scrollLeft;
      if (v !== lastEvtSl.v) {
        trace(`[SCROLL] scrollLeft ${lastEvtSl.v}->${v}`);
        lastEvtSl.v = v;
      }
    };
    container.addEventListener("scroll", onSc, { passive: true });
    onCleanup(() => container.removeEventListener("scroll", onSc));

    // 劫持 scrollLeft setter：判明是“显式赋值”还是“浏览器 clamp(内容塌缩)”
    const desc = Object.getOwnPropertyDescriptor(HTMLDivElement.prototype, "scrollLeft");
    if (desc) {
      Object.defineProperty(container, "scrollLeft", {
        configurable: true,
        get() {
          return desc.get!.call(container);
        },
        set(v) {
          trace(
            `[SET-SL] explicit ${container.scrollLeft}->${v} target=${container.className || container.nodeName}\n${new Error().stack}`,
          );
          return desc.set!.call(container, v);
        },
      });
    }

    // 劫持 scrollTo / scroll：原生方法，不走 scrollLeft setter
    const origScrollTo = (container as any).scrollTo;
    (container as any).scrollTo = function (...args: any[]) {
      trace(
        `[SCROLLTO] sl=${container.scrollLeft} args=${JSON.stringify(args)}\n${new Error().stack}`,
      );
      return origScrollTo.apply(this, args);
    };
    const origScroll = (container as any).scroll;
    (container as any).scroll = function (...args: any[]) {
      trace(
        `[SCROLL] sl=${container.scrollLeft} args=${JSON.stringify(args)}\n${new Error().stack}`,
      );
      return origScroll.apply(this, args);
    };

    // scrollIntoView 也改滚动位置（不走 scrollLeft setter）
    const origSIV = (container as any).scrollIntoView;
    (container as any).scrollIntoView = function (...args: any[]) {
      trace(`[SIV] sl=${container.scrollLeft} args=${JSON.stringify(args)}\n${new Error().stack}`);
      return origSIV.apply(this, args);
    };
    const origWinScroll = window.scrollTo.bind(window);
    (window as any).scrollTo = function (...args: any[]) {
      trace(`[WIN-SCROLL] args=${JSON.stringify(args)}\n${new Error().stack}`);
      return origWinScroll(...args);
    };

    // rAF 逐帧快照：抓 clamp 瞬间的 scrollWidth/clientWidth/contentW + 是否 detached
    const prev = {
      sl: container.scrollLeft,
      sw: container.scrollWidth,
      cw: container.clientWidth,
      conn: container.isConnected,
    };
    let rafId = 0;
    const loop = () => {
      const sl = container.scrollLeft;
      const sw = container.scrollWidth;
      const cw = container.clientWidth;
      const conn = container.isConnected;
      if (conn !== prev.conn) {
        trace(`[CONN] isConnected ${prev.conn}->${conn} sl=${sl}`);
        prev.conn = conn;
      }
      if (sl !== prev.sl || sw !== prev.sw || cw !== prev.cw) {
        const iw = container.firstElementChild?.getBoundingClientRect().width ?? -1;
        trace(
          `[RAF] sl=${prev.sl}->${sl} sw=${prev.sw}->${sw} cw=${prev.cw}->${cw} iw=${iw} dur=${props.duration} zoom=${zoom().toFixed(3)} conn=${conn}`,
        );
        prev.sl = sl;
        prev.sw = sw;
        prev.cw = cw;
      }
      rafId = requestAnimationFrame(loop);
    };
    rafId = requestAnimationFrame(loop);
    onCleanup(() => cancelAnimationFrame(rafId));

    // 全局捕获：任何元素发生 scrollLeft 变化都打点
    const capSc = (e: Event) => {
      const t = e.target as HTMLElement;
      const cur = (t as any).__pid !== undefined ? `pid=${(t as any).__pid}` : "";
      trace(`[SCROLL-capture] ${t.className || t.nodeName} ${cur} sl=${t.scrollLeft}`);
    };
    document.addEventListener("scroll", capSc, true);
    onCleanup(() => document.removeEventListener("scroll", capSc, true));

    // ResizeObserver 抓容器 / 内容宽高瞬时变化
    const ro = new ResizeObserver((entries) => {
      for (const e of entries) {
        const t = e.target as HTMLElement;
        const r = e.contentRect;
        trace(
          `[RESIZE] ${t.className || t.nodeName} w=${Math.round(r.width)} h=${Math.round(r.height)}`,
        );
      }
    });
    if (container) {
      ro.observe(container);
      const inner = container.firstElementChild as HTMLElement | null;
      if (inner) ro.observe(inner);
    }
    onCleanup(() => ro.disconnect());

    const last = { sl: -1, st: -1, dur: -1, ct: -1, tr: -1, w: -1, pid: -1 };
    const id = setInterval(() => {
      const el = tracksRef;
      const sl = el?.scrollLeft ?? -1;
      const st = el?.scrollTop ?? -1;
      const dur = props.duration;
      const ct = props.currentTime;
      const tr = props.tracks.length;
      const w = el ? (el.firstElementChild?.getBoundingClientRect().width ?? -1) : -1;
      const pid = (el as any)?.__pid ?? -1;
      const ch: string[] = [];
      if (sl !== last.sl) {
        ch.push(`sl:${last.sl}->${sl}`);
        last.sl = sl;
      }
      if (st !== last.st) {
        ch.push(`st:${last.st}->${st}`);
        last.st = st;
      }
      if (dur !== last.dur) {
        ch.push(`dur:${last.dur}->${dur}`);
        last.dur = dur;
      }
      if (ct !== last.ct) {
        ch.push(`ct:${last.ct}->${ct}`);
        last.ct = ct;
      }
      if (tr !== last.tr) {
        ch.push(`trks:${last.tr}->${tr}`);
        last.tr = tr;
      }
      if (w !== last.w) {
        ch.push(`contentW:${last.w}->${w}`);
        last.w = w;
      }
      if (pid !== last.pid) {
        ch.push(`pid:${last.pid}->${pid}`);
        last.pid = pid;
      }
      if (ch.length) trace(`[TRACE]`, ch.join(" | "));
    }, 50);
    onCleanup(() => clearInterval(id));

    const mo = new MutationObserver((list) => {
      for (const m of list) {
        const tgt = m.target as HTMLElement;
        if (m.type === "childList") {
          const added = m.addedNodes.length;
          const removed = m.removedNodes.length;
          if (added || removed)
            trace(
              `[TRACE-mut] childList on ${tgt.className || tgt.nodeName} +${added} -${removed}`,
            );
        } else if (m.type === "attributes") {
          const newVal = (tgt as any).getAttribute(m.attributeName);
          trace(
            `[TRACE-mut] attr:${m.attributeName}="${newVal}" on ${tgt.className || tgt.nodeName}`,
          );
        }
      }
    });
    // 同时盯容器本身和它的父级，抓“滚动容器被替换”；并盯 style/class 突变
    if (container) {
      mo.observe(container, {
        childList: true,
        attributes: true,
        attributeFilter: ["style", "class", "hidden", "display"],
      });
      if (container.parentElement) mo.observe(container.parentElement, { childList: true });
    }
    onCleanup(() => mo.disconnect());
  });

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
    // anchor playhead position in viewport
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

  return (
    <div class="flex flex-col h-full  border-t select-none">
      <TimelineToolbar zoom={zoom()} sliderValue={sliderPos()} onSliderChange={onSliderChange} />

      <div class="flex flex-1 overflow-hidden">
        <TimelineTrackSide
          ref={(el) => (labelsRef = el)}
          tracks={props.tracks}
          trackColor={trackColor}
          taskDir={props.taskDir ?? ""}
        />

        <div ref={rightRef!} class="flex-1 flex flex-col min-w-0 relative overflow-hidden">
          <TimelineRuler
            ref={(el) => (rulerRef = el)}
            totalPx={totalPx()}
            duration={props.duration}
            rulerCfg={rc()}
            pxPerMs={pxPerMs()}
            fps={fpsFloat()}
            onSeek={props.onSeek}
          />

          <TimelineTracks
            ref={(el) => (tracksRef = el)}
            tracks={props.tracks}
            totalPx={totalPx()}
            pxPerMs={pxPerMs()}
            onSeek={props.onSeek}
            trackColor={trackColor}
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
