import { get_group_list, getTaskCtx } from "./get_task";
import { InputArgs } from "../../input/input";
import { cmdRerunStage } from "./rerunStage";
import { cmdResumeTask } from "./resumeTask";
import { cmdStartTask } from "./startTask";
import { cmdTaskStatus } from "./taskStatus";
import { readCtx } from "@repo/core/context/context";
import { getStageStatuses } from "@repo/core/stages/utils/utils";

export const cmdTask = async (input: InputArgs) => {
  if (input.task.action === "resume") {
    await cmdResumeTask(input);
  } else if (input.task.action === "start") {
    await cmdStartTask(input);
  } else if (input.task.action === "rerun_stage") {
    await cmdRerunStage(input);
  } else if (input.task.action === "status") {
    const taskDir = input.task?.taskDir!;
    await readCtx(taskDir);
    const status = await getStageStatuses(taskDir);
    console.log(status);
  } else if (input.task.action === "get_group_list") {
    const group_list = await get_group_list();
    console.log(JSON.stringify(group_list, null, 2));
  } else if (input.task.action === "get_task_ctx") {
    const taskDir = input.task?.taskDir!;
    const ctx = getTaskCtx(taskDir);
    console.log(JSON.stringify(ctx, null, 2));
  } else {
    console.error(`Unknown task action: ${input.task.action}`);
  }
};
