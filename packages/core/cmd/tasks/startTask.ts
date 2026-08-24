import { InputArgs } from "@repo/core/input/input";
import { importVideo } from "@repo/core/tasks/import/download";
import { playTaskFail, playTaskSuccess } from "@repo/core/cmd/tasks/utils";
import { runPipeline } from "../../tasks/start";

export const cmdStartTask = async (input: InputArgs) => {
  const args = input.task;
  const url = args.url;
  if (!url) {
    console.error("task start: need task.url in input.json");
    process.exit(1);
  }
  const ctx = await importVideo(input);
  try {
    console.log(`\n[CLI] Running pipeline ...`);
    await runPipeline(ctx);
    console.log("[CLI] Pipeline success");
    playTaskSuccess();
    process.exit(0);
  } catch (err) {
    console.error("cmdCreateTask failed:", err);
    playTaskFail();
    process.exit(1);
  }
};
