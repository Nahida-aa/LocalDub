import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { REPO_ROOT, WORKFOLDER } from '@repo/config';
import { to } from '@repo/shared/lib/utils/try.ts';
// import { eq, sql } from 'drizzle-orm';
import { getStages } from './../../feat/tasks/stages.ts';
// import { taskStages, tasks } from './../../feat/tasks/table.ts';
import type { MLDaemon } from '../../ml/daemon/client.ts';
import {
	readConfig,
} from '../config/config.ts';
import type { Ctx } from '../config/types.ts';
import { STAGE_HANDLERS } from '../stages/index.ts';
import { Context, 	readCtx,
	readPipeline,
	readTask,
	setCtx,
	setStage,
	setTask,
	writeCtx,
    writeStages,  listStage, } from '../context/context.ts';
import {
	emitLog,
	getStageStatuses,
	nowISO,
	// updateStageDB,
	// updateTaskDB,
} from '../stages/utils/utils.ts';

export { getStageStatuses };

function snapshotConfig(sessionPath: string) {
	const cfg = readConfig();
	const snap: NonNullable<Context['input']> = {
		...cfg,
		timestamp: new Date().toISOString(),
		pipeline: cfg.pipeline ?? 'dub',
	};

	setCtx(sessionPath, { input: snap });
}

export async function runPipeline(ctx: Context, daemon?: MLDaemon) {
	const taskId= ctx.task.id
	const sessionPath = ctx.task.session_path
	let task = readTask(sessionPath);
	mkdirSync(sessionPath, { recursive: true });

	const pipeline = readPipeline(sessionPath);
	const stages = getStages(pipeline);
	const targetStage = readConfig().targetStage;
	if (targetStage && !stages.find((s) => s.name === targetStage)) {
		emitLog(sessionPath, `[WARN] targetStage "${targetStage}" 不在 ${pipeline} pipeline 中，忽略`);
	}

	snapshotConfig(sessionPath);

	await setTask(sessionPath, { status: 'running', started_at: nowISO() });

	for (const stage of stages) {
		setTask(sessionPath, {
			current_stage: stage.name
		});
		const handler = STAGE_HANDLERS[stage.name];
		if (!handler) {
			emitLog(
				sessionPath,
				`[WARN] [Pipeline] No handler for stage ${stage.name}, skipping`,
			);
			continue;
		}

		await setStage(sessionPath, stage.name, {
			status: 'running',
			started_at: nowISO(),
			last_message: `Starting ${stage.label}...`,
		});
		await setTask(sessionPath, { current_stage: stage.name });

		try {
			await handler(taskId, sessionPath, task, daemon);
			if (targetStage && stage.name === targetStage) {
				emitLog(sessionPath, `[Pipeline] 达到目标步骤 "${targetStage}"，停止`);
				break;
			}
		} catch (err: any) {
			const msg = err.message ?? String(err);
			emitLog(sessionPath, `[ERROR] [Pipeline] Stage ${stage.name} failed: ${msg}`);
			await setStage(sessionPath, stage.name, {
				status: 'failed',
				error_message: msg,
				completed_at: nowISO(),
			});
			await setTask(sessionPath, { status: 'failed', error_message: msg });
			throw err;
		}

		const next = readTask(sessionPath)
		if (next) {
			task = next;
		}
	}

	setTask(sessionPath, {
		status: 'succeeded',
		completed_at: nowISO(),
		current_stage: null,
	});
	emitLog(sessionPath, `[Pipeline] Task ${taskId} completed`);
}

export async function resumePipeline(
	ctx: Context,
	resumeFrom?: string,
	stageOverrides?: Record<string, any>,
	daemon?: MLDaemon,
) {
		const taskId= ctx.task.id
	const sessionPath = ctx.task.session_path
	let task = readTask(sessionPath);

	const [info, err] = to(() => readCtx(sessionPath));
	if (err) {
		throw new Error(`Failed to read local info: ${err.message}`);
	}
	// Mode transition handling
	const lastRunMode = info.lastRunPipeline;
	if (lastRunMode && lastRunMode !== info.pipeline) {
		info.lastRunPipeline = info.pipeline;
		emitLog(
			sessionPath,
			`[Pipeline] switched from "${lastRunMode}" to "${info.pipeline}"`,
		);

		const stages = getStages(info.pipeline);
		const existing = listStage(sessionPath);
		const existingNames = new Set(existing.map((r) => r.name));
		const newStages = stages.filter((s) => !existingNames.has(s.name));
		if (newStages.length > 0) {

			writeStages(sessionPath,
				newStages.map((s) => ({
					task_id: taskId,
					name: s.name,
					label: s.label,
					status: 'pending',
				})),
			);
		}
		// merge_video produces different output per pipeline → force re-run
		setStage(sessionPath, 'merge_video', {
							status: 'pending',
				started_at: null,
				completed_at: null,
				error_message: null,
				progress: 0,
		})

	}

	writeCtx(info);

	snapshotConfig(sessionPath);

	const pipeline = info.pipeline || 'dub';
	const stages = getStages(pipeline);

	let startIdx = 0;

	if (resumeFrom) {
		startIdx = stages.findIndex((s) => s.name === resumeFrom);
		if (startIdx === -1) throw new Error(`Unknown stage "${resumeFrom}"`);
		for (let i = startIdx; i < stages.length; i++) {
			setStage(sessionPath, stages[i].name, {
				status: 'pending',
				started_at: null,
				completed_at: null,
				error_message: null,
				progress: 0,
			});
		}
		emitLog(
			sessionPath,
			`[Pipeline] Resetting from "${resumeFrom}" (${stages.length - startIdx} stage(s)), resuming...`,
		);
	} else {
		const rows = listStage(sessionPath);
		const stageStatus = new Map(rows.map((r) => [r.name, r.status]));

		for (let i = 0; i < stages.length; i++) {
			if (stageStatus.get(stages[i].name) !== 'succeeded') {
				startIdx = i;
				break;
			}
		}

		if (startIdx === 0) {
			emitLog(sessionPath, `[Pipeline] Resuming from beginning`);
		} else {
			emitLog(
				sessionPath,
				`[Pipeline] Skipping ${startIdx} completed stage(s), resuming from "${stages[startIdx].name}"`,
			);
		}
	}

	const resumeTargetStage = readConfig().targetStage;
	if (resumeTargetStage && !stages.find((s) => s.name === resumeTargetStage)) {
		emitLog(sessionPath, `[WARN] targetStage "${resumeTargetStage}" 不在 ${pipeline} pipeline 中，忽略`);
	}

	setTask(sessionPath, { status: 'running', started_at: nowISO() });

	for (let i = startIdx; i < stages.length; i++) {
		const stage = stages[i];
		setTask(sessionPath, {
			current_stage: stage.name
		});
		const handler = STAGE_HANDLERS[stage.name];
		if (!handler) {
			emitLog(
				sessionPath,
				`[WARN] [Pipeline] No handler for stage ${stage.name}, skipping`,
			);
			continue;
		}

		setStage(sessionPath, stage.name, {
			status: 'running',
			started_at: nowISO(),
			last_message: `Starting ${stage.label}...`,
		});
		setTask(sessionPath, { current_stage: stage.name });

		try {
			await handler(taskId, sessionPath, task, daemon);
			if (resumeTargetStage && stage.name === resumeTargetStage) {
				emitLog(sessionPath, `[Pipeline] 达到目标步骤 "${resumeTargetStage}"，停止`);
				break;
			}
		} catch (err: any) {
			const msg = err.message ?? String(err);
			emitLog(sessionPath, `[ERROR] [Pipeline] Stage ${stage.name} failed: ${msg}`);
			setStage(sessionPath, stage.name, {
				status: 'failed',
				error_message: msg,
				completed_at: nowISO(),
			});
			await setTask(sessionPath, { status: 'failed', error_message: msg });
			throw err;
		}

		const next = readTask(sessionPath)
		if (next) {
			task = next;
		}
	}

	setTask(sessionPath, {
		status: 'succeeded',
		completed_at: nowISO(),
		current_stage: null,
	});
	emitLog(sessionPath, `[Pipeline] Task ${taskId} completed`);
}

export async function rerunSingleStage(
	ctx: Context,
	stageName: string,
	stageOverrides?: Record<string, any>,
	daemon?: MLDaemon,
) {
	const taskId= ctx.task.id
	const sessionPath = ctx.task.session_path
	const task = readTask(sessionPath)

	const pipeline = readPipeline(sessionPath);
	const stages = getStages(pipeline);

	const stage = stages.find((s) => s.name === stageName);
	if (!stage) throw new Error(`Unknown stage "${stageName}"`);

	const handler = STAGE_HANDLERS[stageName];
	if (!handler) throw new Error(`No handler for stage "${stageName}"`);

	snapshotConfig(sessionPath);

	setStage(sessionPath, stageName, {
		status: 'running',
		completed_at: null,
		error_message: null,
		started_at: nowISO(),
		progress: 0,
		last_message: `Rerunning ${stage.label}...`,
	});

	setTask(sessionPath, { status: 'running', current_stage: stageName });

	try {
		await handler(taskId, sessionPath, task, daemon);
	} catch (err: any) {
		const msg = err.message ?? String(err);
		emitLog(sessionPath, `[ERROR] [Pipeline] Stage ${stageName} failed: ${msg}`);
		await setStage(sessionPath, stageName, {
			status: 'failed',
			error_message: msg,
			completed_at: nowISO(),
		});
		await setTask(sessionPath, { status: 'failed', error_message: msg });
		throw err;
	}

	await setStage(sessionPath, stageName, {
		status: 'succeeded',
		completed_at: nowISO(),
		progress: 100,
		last_message: `${stage.label} completed`,
	});
	emitLog(sessionPath, `[Pipeline] Stage ${stageName} completed`);
}
