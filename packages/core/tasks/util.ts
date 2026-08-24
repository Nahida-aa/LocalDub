import { readInputArgs } from "@repo/core/input/input";
import { TaskCtx, setCtx, _writeCtx } from "@repo/core/context/context.ts";

export function snapshotInput(taskDir: string) {
  const args = readInputArgs();

  const snap: NonNullable<TaskCtx["input"]> = {
    ...args,
    timestamp: new Date().toISOString(),
    pipeline: args.task.pipeline ?? "dub",
  };

  setCtx(taskDir, { input: snap });
}
