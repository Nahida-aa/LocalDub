import { InputArgs } from "@repo/core/input/input";
import { rerunSingleStage } from "../../tasks/pipeline-runner";
import { setCtx } from "@repo/core/context/context";

export const cmdRerunStage = async (input: InputArgs) => {
	const taskDir = input.task?.taskDir;
	const stageName = input.task?.stageName;
	if (!taskDir || !stageName) {
		console.error(
			'rerunStage.taskDir and rerunStage.stageName required in input.json',
		);
		throw new Error('rerunStage.taskDir and rerunStage.stageName required in input.json');
	}
	const ctx = setCtx(taskDir, { input });
	const taskId = ctx.task.id;
	console.log(`[CLI] Rerunning stage "${stageName}" for task ${taskId}...`);
	await	rerunSingleStage(ctx),
	console.log('[CLI] Stage completed');
}