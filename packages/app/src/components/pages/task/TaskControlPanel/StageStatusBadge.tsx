import { type Component, Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import { CheckCircle2, CircleCheck, CircleDashed, CircleX, LoaderCircle } from "lucide-solid";
import type { StageStatus } from "@repo/core/context/types";
import { TooltipX } from "@repo/ui-solid/custom/tooltip";
import { cn } from "@repo/shared/lib/utils";

type StatusConfig = {
  label: string;
  icon: Component<{ class?: string }>;
  class: string;
};

const STATUS_CONFIG: Record<StageStatus, StatusConfig> = {
  pending: {
    label: "待运行",
    icon: CircleDashed,
    class: "text-muted-foreground",
  },
  running: {
    label: "运行中",
    icon: LoaderCircle,
    class: "text-blue-400",
  },
  success: {
    label: "已完成",
    icon: CircleCheck,
    class: "text-green-500",
  },
  failed: {
    label: "失败",
    icon: CircleX,
    class: "text-destructive",
  },
};

export const StageStatusBadge: Component<{
  status: StageStatus;
  progress?: number | null;
}> = (p) => {
  const cfg = () => STATUS_CONFIG[p.status];
  const pct = () => Math.min(100, Math.max(0, Math.round(p.progress ?? 0)));
  const tip = () =>
    p.status === "running" && p.progress != null ? `${cfg().label} ${pct()}%` : cfg().label;
  return (
    <TooltipX content={tip()}>
      <span class={cn("relative flex items-center", cfg().class)}>
        <Dynamic component={cfg().icon} class="size-3.5" />
        <Show when={p.status === "running" && p.progress != null}>
          <span class="absolute -bottom-1 left-0 w-10 h-0.5 rounded overflow-hidden bg-muted">
            <span class="block h-full bg-blue-400 transition-all" style={{ width: `${pct()}%` }} />
          </span>
        </Show>
      </span>
    </TooltipX>
  );
};
