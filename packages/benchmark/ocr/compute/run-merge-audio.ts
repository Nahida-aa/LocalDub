import { resolve } from "node:path";
import { stageMixAudio } from "@repo/core/stages/mix_audio/mix_audio";
import type { TaskCtx } from "@repo/core/context/context";

const label = process.argv[2];
if (!label) {
  console.error("Usage: bun run-merge-audio.ts <results/label>");
  process.exit(1);
}

const ctxPath = resolve(__dirname, "..", "results", label, "metadata", "ctx.json");
const ctx: TaskCtx = JSON.parse(await Bun.file(ctxPath).text());

await stageMixAudio(ctx);
