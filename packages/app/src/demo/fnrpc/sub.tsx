import { fnrpc } from "#/integrations/fnrpc/client.ts";
import { consumeEventIterator } from "@fnrpc/client";
import { createEffect, createSignal, onCleanup, Show } from "solid-js";

function Row(props: { label: string; children: any }) {
  return (
    <div class="flex items-center gap-2 flex-wrap min-h-9 px-3 py-2 rounded-lg bg-card border text-sm">
      <span class="font-mono text-xs text-muted-foreground shrink-0 w-36">{props.label}</span>
      {props.children}
    </div>
  );
}
function TickTest() {
  const [count, setCount] = createSignal<string | null>(null);
  const [running, setRunning] = createSignal(false);

  createEffect(() => {
    if (!running()) return;
    const iter = fnrpc.watch_task_log("");
    const cancel = consumeEventIterator(iter, {
      onEvent: (v) => {
        setCount(v);
      },
      onError: (e) => {
        console.error("watch_task_log error", e);
      },
    });
    onCleanup(() => cancel());
  });

  return (
    <Row label="watch_task_log('')">
      <span class="text-muted-foreground text-xs">repo_root/.log</span>
      <button
        class={
          running()
            ? "bg-red-600 text-white px-3 py-1 rounded text-sm hover:bg-red-700"
            : "bg-green-600 text-white px-3 py-1 rounded text-sm hover:bg-green-700"
        }
        onClick={() => setRunning(!running())}
      >
        {running() ? "Stop" : "Start"}
      </button>
      <Show when={count() !== null}>
        <span class="font-mono text-sm">Value: {count()}</span>
      </Show>
    </Row>
  );
}
