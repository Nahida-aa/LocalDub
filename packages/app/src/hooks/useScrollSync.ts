import { onMount, onCleanup } from "solid-js";

export function useScrollSync(
  getTracks: () => HTMLDivElement | undefined,
  getRuler: () => HTMLDivElement | undefined,
  getLabels: () => HTMLDivElement | undefined,
) {
  onMount(() => {
    const tracks = getTracks();
    const ruler = getRuler();
    const labels = getLabels();
    if (!tracks) return;

    // 轨道滚动 → 带动 ruler(横向) 与 标签列(纵向)
    function syncFromTracks() {
      if (ruler) ruler.scrollLeft = tracks!.scrollLeft;
      if (labels) labels.scrollTop = tracks!.scrollTop;
    }

    // 标签列滚动 → 反向带动轨道(纵向)，双向联动
    function syncFromLabels() {
      if (labels) tracks!.scrollTop = labels.scrollTop;
    }

    tracks.addEventListener("scroll", syncFromTracks, { passive: true });
    labels?.addEventListener("scroll", syncFromLabels, { passive: true });

    onCleanup(() => {
      tracks.removeEventListener("scroll", syncFromTracks);
      labels?.removeEventListener("scroll", syncFromLabels);
    });
  });
}
