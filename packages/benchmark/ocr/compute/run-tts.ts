import { resolve } from "node:path";
import { stageTts } from "@repo/core/stages/07_tts/tts";
import { TaskCtx } from "@repo/core/context/context";

const label = process.argv[2];
if (!label) {
  console.error("Usage: bun run-tts.ts <results/label>");
  process.exit(1);
}

const ctxPath = resolve(__dirname, "..", "results", label, "metadata", "ctx.json");
const ctx: TaskCtx = JSON.parse(await Bun.file(ctxPath).text());

await stageTts(ctx);
