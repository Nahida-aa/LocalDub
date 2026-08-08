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
  getStageStatuses,
  nowISO,
  // updateStageDB,
  // updateTaskDB,
} from "@repo/core/stages/utils/utils.ts";

function snapshotConfig(taskDir: string) {
  const args = readInputArgs();

  const snap: NonNullable<TaskCtx["input"]> = {
    ...args,
    timestamp: new Date().toISOString(),
    pipeline: args.task.pipeline ?? "dub",
  };

  setCtx(taskDir, { input: snap });
}

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
    emitLog(taskDir, `[WARN] targetStage "${targetStage}" 不在 ${pipeline} pipeline 中，忽略`);
  }

  snapshotConfig(taskDir);

  await setTask(taskDir, { status: "running", started_at: nowISO() });

  for (const stage of stages) {
    const handler = STAGE_HANDLERS[stage];
    if (!handler) {
      emitLog(taskDir, `[WARN] [Pipeline] No handler for stage ${stage}, skipping`);
      continue;
    }

    await setStage(taskDir, stage, {
      status: "running",
      started_at: nowISO(),
      last_message: `Starting ${stage}...`,
    });
    setTask(taskDir, { status: "running", current_stage: stage, started_at: nowISO() });
    try {
      await handler(taskDir);
      if (targetStage && stage === targetStage) {
        emitLog(taskDir, `[Pipeline] 达到目标步骤 "${targetStage}"，停止`);
        break;
      }
    } catch (err: any) {
      const msg = err.message ?? String(err);
      emitLog(taskDir, `[ERROR] [Pipeline] Stage ${stage} failed: ${msg}`);
      await setStage(taskDir, stage, {
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
  emitLog(taskDir, `[Pipeline] Task ${taskId} completed`);
}

export async function resumePipeline(ctx: TaskCtx) {
  const taskDir = ctx.task.task_dir;
  setLogContext(taskDir);
  const resumeFrom = ctx.input?.task?.resumeFrom;
  ctx.task.current_stage = "resumePipeline";
  setTask(ctx.task.task_dir, { current_stage: "resumePipeline" });
  const taskId = ctx.task.id;
  let task = readTask(taskDir);
  // Mode transition handling
  const lastRunMode = ctx.lastRunPipeline;
  if (lastRunMode && lastRunMode !== ctx.pipeline) {
    ctx.lastRunPipeline = ctx.pipeline;
    emitLog(taskDir, `[Pipeline] switched from "${lastRunMode}" to "${ctx.pipeline}"`);

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
    // merge_video produces different output per pipeline → force re-run
    setStage(taskDir, "merge_video", {
      status: "pending",
      started_at: null,
      completed_at: null,
      error_message: null,
      progress: 0,
    });
  }

  _writeCtx(ctx);

  snapshotConfig(taskDir);

  const pipeline = ctx.pipeline || "dub";
  const stages = getStages(pipeline);

  let startIdx = 0;

  if (resumeFrom) {
    startIdx = stages.findIndex((s) => s === resumeFrom);
    if (startIdx === -1) throw new Error(`Unknown stage "${resumeFrom}"`);
    for (let i = startIdx; i < stages.length; i++) {
      setStage(taskDir, stages[i], {
        status: "pending",
        started_at: null,
        completed_at: null,
        error_message: null,
        progress: 0,
      });
    }
    emitLog(
      taskDir,
      `[Pipeline] Resetting from "${resumeFrom}" (${stages.length - startIdx} stage(s)), resuming...`,
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
      emitLog(taskDir, `[Pipeline] Resuming from beginning`);
    } else {
      emitLog(
        taskDir,
        `[Pipeline] Skipping ${startIdx} completed stage(s), resuming from "${stages[startIdx]}"`,
      );
    }
  }

  const resumeTargetStage = ctx.input?.task?.targetStage;
  if (resumeTargetStage && !stages.find((s) => s === resumeTargetStage)) {
    emitLog(
      taskDir,
      `[WARN] targetStage "${resumeTargetStage}" 不在 ${pipeline} pipeline 中，忽略`,
    );
  }

  // 计算出目标步骤的索引
  const targetIdx = resumeTargetStage ? stages.findIndex((s) => s === resumeTargetStage) : -1;
  // 计算出 要运行的 stage 列表
  const runStages = targetIdx >= 0 ? stages.slice(startIdx, targetIdx + 1) : stages.slice(startIdx);
  console.log(`[Pipeline] Running runStages:`, runStages);
  for (let i = startIdx; i < stages.length; i++) {
    const stage = stages[i];
    const handler = STAGE_HANDLERS[stage];
    if (!handler) {
      emitLog(taskDir, `[WARN] [Pipeline] No handler for stage ${stage}, skipping`);
      continue;
    }

    setStage(taskDir, stage, {
      status: "running",
      started_at: nowISO(),
      last_message: `Starting ${stage}...`,
    });
    setTask(taskDir, { status: "running", current_stage: stage, started_at: nowISO() });
    try {
      await handler(taskDir);
      if (resumeTargetStage && stage === resumeTargetStage) {
        emitLog(taskDir, `[Pipeline] 达到目标步骤 "${resumeTargetStage}"，停止`);
        break;
      }
    } catch (err: any) {
      const msg = err.message ?? String(err);
      emitLog(taskDir, `[ERROR] [Pipeline] Stage ${stage} failed: ${msg}`);
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
  emitLog(taskDir, `[Pipeline] Task ${taskId} completed`);
}

export async function rerunSingleStage(ctx: TaskCtx) {
  const taskId = ctx.task.id;
  const taskDir = ctx.task.task_dir;
  setLogContext(taskDir);
  const task = readTask(taskDir);
  const stageName = ctx.input?.task.stageName;
  const pipeline = readPipeline(taskDir);
  const stages = getStages(pipeline);

  const stage = stages.find((s) => s === stageName);
  if (!stage) throw new Error(`Unknown stage "${stageName}"`);

  const handler = STAGE_HANDLERS[stage];
  if (!handler) throw new Error(`No handler for stage "${stageName}"`);

  snapshotConfig(taskDir);

  setStage(taskDir, stage, {
    status: "running",
    completed_at: null,
    error_message: null,
    started_at: nowISO(),
    progress: 0,
    last_message: `Rerunning ${stage}...`,
  });
  setTask(taskDir, { status: "running", current_stage: stage, started_at: nowISO() });
  try {
    await handler(taskDir);
  } catch (err: any) {
    const msg = err.message ?? String(err);
    emitLog(taskDir, `[ERROR] [Pipeline] Stage ${stageName} failed: ${msg}`);
    await setStage(taskDir, stage, {
      status: "failed",
      error_message: msg,
      completed_at: nowISO(),
    });
    await setTask(taskDir, { status: "failed", error_message: msg });
    throw err;
  }

  await setStage(taskDir, stage, {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
    last_message: `${stage} completed`,
  });
  emitLog(taskDir, `[Pipeline] Stage ${stageName} completed`);
}
