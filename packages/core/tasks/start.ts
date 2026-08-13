import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { to } from "@repo/shared/lib/utils/try.ts";
// import { eq, sql } from 'drizzle-orm';
import { getStages } from "@repo/core/stages/utils/stages";
// import { taskStages, tasks } from './../../feat/tasks/table.ts';
import { readInputArgs } from "@repo/core/input/input";
import { STAGE_HANDLERS } from "../stages/index";
import {
  TaskCtx,
  readCtx,
  readPipeline,
  readTask,
  setCtx,
  setStage,
  setTask,
  writeCtx,
  writeStages,
  listStage,
  _writeCtx,
} from "@repo/core/context/context.ts";
import {
  emitLog,
  setLogContext,
  setCurrentStage,
  getStageStatuses,
  nowISO,
  // updateStageDB,
  // updateTaskDB,
} from "@repo/core/stages/utils/utils.ts";
import { log } from "@repo/util/log";
import { snapshotInput } from "./util";

export async function runPipeline(ctx: TaskCtx) {
  const taskId = ctx.task.id;
  const taskDir = ctx.task.task_dir;
  setLogContext(taskDir);
  let task = readTask(taskDir);
  mkdirSync(taskDir, { recursive: true });

  const pipeline = readPipeline(taskDir);
  const stages = getStages(pipeline);
  const targetStage = ctx.input?.targetStage;
  if (targetStage && !stages.find((s) => s === targetStage)) {
    log(`[WARN] targetStage "${targetStage}" 不在 ${pipeline} pipeline 中，忽略`);
  }

  snapshotInput(taskDir);

  setTask(taskDir, { status: "running", started_at: nowISO() });

  for (const stage of stages) {
    const handler = STAGE_HANDLERS[stage];
    if (!handler) {
      log(`[WARN] [Pipeline] No handler for stage ${stage}, skipping`);
      continue;
    }

    setStage(taskDir, stage, {
      status: "running",
      started_at: nowISO(),
      last_message: `Starting ${stage}...`,
    });
    setTask(taskDir, { status: "running", current_stage: stage, started_at: nowISO() });
    setCurrentStage(stage);
    try {
      await handler(taskDir);
      if (targetStage && stage === targetStage) {
        log(`[Pipeline] 达到目标步骤 "${targetStage}"，停止`);
        break;
      }
    } catch (err: any) {
      const msg = err.message ?? String(err);
      log(`[ERROR] [Pipeline] Stage ${stage} failed: ${msg}`);
      setStage(taskDir, stage, {
        status: "failed",
        error_message: msg,
        completed_at: nowISO(),
      });
      setTask(taskDir, { status: "failed", error_message: msg });
      throw err;
    }

    const next = readTask(taskDir);
    if (next) {
      task = next;
    }
  }

  setTask(taskDir, {
    status: "success",
    completed_at: nowISO(),
    current_stage: null,
  });
  log(`[Pipeline] Task ${taskId} completed`);
}
