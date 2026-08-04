import { InputArgs } from "@repo/core/input/input";
import { resumePipeline } from "../../tasks/pipeline-runner";
import { playTaskFail, playTaskSuccess } from "./utils";
import { setCtx } from "@repo/core/context/context";

export const cmdResumeTask = async (input: InputArgs) => {
  const taskDir = input.task?.taskDir;
		if (!taskDir) {
			console.error('task.taskDir required in input.json');
			throw new Error('task.taskDir required in input.json');
		}
		const ctx =  setCtx(taskDir, {
			input: input,
		});
		const taskId = ctx.task.id;
		const resumeFrom = input.task?.resumeFrom;
		const label = resumeFrom ? ` from "${resumeFrom}"` : '';

		console.log(`[CLI] Resuming pipeline for task ${taskDir}${label}...`);
		await	resumePipeline(ctx),
		console.log('[CLI] Pipeline completed');
		playTaskSuccess()
}