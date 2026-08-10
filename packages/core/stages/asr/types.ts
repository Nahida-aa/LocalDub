import { TaskCtx } from "@repo/core/context/context";

export interface AsrOptions {
  ctx: TaskCtx;
  taskId: string;
  audioPath: string;
  taskDir: string;
  language?: string;
  device: string;
  pythonBin: string;
}
