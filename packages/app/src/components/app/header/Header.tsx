import { cn } from "@repo/shared/lib/utils";
import { buttonVariants } from "@repo/ui-solid/base/button";
import { TooltipX } from "@repo/ui-solid/custom/tooltip";
import { useParams } from "@tanstack/solid-router";
import { SquareTerminal } from "lucide-solid";
import { StartWorkflowDialog } from "./StartWorkflowDialog";

export const Header = () => {
  const p = useParams({ strict: false });
  const activeTitle = () =>
    p().id ? (p().workflowId ? `${p().id}/${p().workflowId}` : p().id) : "";
  return (
    <header class="px-3 h-10 py-2 border-b">
      <div class=" flex min-w-0 flex-1 items-center gap-2 sm:gap-3">
        <div class="flex min-w-0 flex-1 items-center gap-2 overflow-hidden sm:gap-3">
          {activeTitle()}
        </div>
        <div
          data-header-actions
          class={cn("flex shrink-0 items-center justify-end gap-2 @3xl/header-actions:gap-3")}
        >
          <StartWorkflowDialog />
          <TooltipX
            content={`Toggle terminal drawer`}
            class={buttonVariants({ variant: "icon", size: "xs" })}
          >
            <SquareTerminal size={16} />
          </TooltipX>
        </div>
      </div>
    </header>
  );
};
