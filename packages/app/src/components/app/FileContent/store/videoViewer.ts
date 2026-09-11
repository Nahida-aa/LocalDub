import { createStore, useSelector } from "@tanstack/solid-store";
import type { FrameRate } from "@repo/util/timecode";

interface VideoViewerStore {
  currentTime: number;
  duration: number;
  fps: FrameRate;
  playing: boolean;
  playbackRate: number;
}

export const videoViewerStore = createStore<VideoViewerStore>({
  currentTime: 0,
  duration: 0,
  fps: { numerator: 30, denominator: 1 },
  playing: false,
  playbackRate: 1,
});

export const useCurrentTime = () => useSelector(videoViewerStore, (state) => state.currentTime);
export const useDuration = () => useSelector(videoViewerStore, (state) => state.duration);
export const useFps = () => useSelector(videoViewerStore, (state) => state.fps);
export const usePlaying = () => useSelector(videoViewerStore, (state) => state.playing);
export const usePlaybackRate = () => useSelector(videoViewerStore, (state) => state.playbackRate);

export const setCurrentTime = (currentTime: number) =>
  videoViewerStore.setState((state) => ({ ...state, currentTime }));
export const setDuration = (duration: number) =>
  videoViewerStore.setState((state) => ({ ...state, duration }));
export const setFps = (fps: FrameRate) => videoViewerStore.setState((state) => ({ ...state, fps }));
export const setPlaying = (playing: boolean) =>
  videoViewerStore.setState((state) => ({ ...state, playing }));
export const setPlaybackRate = (playbackRate: number) =>
  videoViewerStore.setState((state) => ({ ...state, playbackRate }));
