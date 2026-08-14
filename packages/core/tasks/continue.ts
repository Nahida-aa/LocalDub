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

export async function continuePipeline(ctx: TaskCtx) {
  const taskDir = ctx.task.task_dir;
  setLogContext(taskDir);
  const continueFrom = ctx.input?.task?.continueFrom;
  ctx.task.current_stage = "resumePipeline";
  setTask(ctx.task.task_dir, { current_stage: "resumePipeline" });
  const taskId = ctx.task.id;
  let task = readTask(taskDir);
  // Mode transition handling
  const lastRunMode = ctx.lastRunPipeline;
  if (lastRunMode && lastRunMode !== ctx.pipeline) {
    ctx.lastRunPipeline = ctx.pipeline;
    log(`[Pipeline] switched from "${lastRunMode}" to "${ctx.pipeline}"`);

    const stages = getStages(ctx.pipeline);
    const existing = listStage(taskDir);
    const existingNames = new Set(existing.map((r) => r.name));
    const newStages = stages.filter((s) => !existingNames.has(s));
    if (newStages.length > 0) {
      writeStages(
        taskDir,
        newStages.map((s) => ({
          task_id: taskId,
          name: s,
          label: s,
          status: "pending",
        })),
      );
    }
    // mix_video produces different output per pipeline → force re-run
    setStage(taskDir, "mix_video", {
      status: "pending",
      started_at: null,
      completed_at: null,
      error_message: null,
      progress: 0,
    });
  }

  _writeCtx(ctx);

  snapshotInput(taskDir);

  const pipeline = ctx.pipeline || "dub";
  const stages = getStages(pipeline);

  let startIdx = 0;

  if (continueFrom) {
    startIdx = stages.findIndex((s) => s === continueFrom);
    if (startIdx === -1) throw new Error(`Unknown stage "${continueFrom}"`);
    for (let i = startIdx; i < stages.length; i++) {
      setStage(taskDir, stages[i], {
        status: "pending",
        started_at: null,
        completed_at: null,
        error_message: null,
        progress: 0,
      });
    }
    log(
      `[Pipeline] Resetting from "${continueFrom}" (${stages.length - startIdx} stage(s)), resuming...`,
    );
  } else {
    const rows = listStage(taskDir);
    const stageStatus = new Map(rows.map((r) => [r.name, r.status]));

    for (let i = 0; i < stages.length; i++) {
      if (stageStatus.get(stages[i]) !== "success") {
        startIdx = i;
        break;
      }
    }

    if (startIdx === 0) {
      log(`[Pipeline] continue from beginning`);
    } else {
      log(
        `[Pipeline] Skipping ${startIdx} completed stage(s), resuming from "${stages[startIdx]}"`,
      );
    }
  }

  const targetStage = ctx.input?.task?.targetStage;
  if (targetStage && !stages.find((s) => s === targetStage)) {
    log(`[WARN] targetStage "${targetStage}" 不在 ${pipeline} pipeline 中，忽略`);
  }

  // 计算出目标步骤的索引
  const targetIdx = targetStage ? stages.findIndex((s) => s === targetStage) : -1;
  // 计算出 要运行的 stage 列表
  const runStages = targetIdx >= 0 ? stages.slice(startIdx, targetIdx + 1) : stages.slice(startIdx);
  console.log(`[Pipeline] Running runStages:`, runStages);
  for (let i = startIdx; i < stages.length; i++) {
    const stage = stages[i];
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
      await setTask(taskDir, { status: "failed", error_message: msg });
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
