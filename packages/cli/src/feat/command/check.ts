import { existsSync, readFileSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { eq } from 'drizzle-orm';
import { REPO_ROOT, WORKFOLDER } from '@repo/config';
import { db } from '../../db/index.ts';
import { tasks } from '../tasks/table.ts';

export async function cmdCheck(type: 'video' | 'asr' | undefined, taskId: string) {
	const rows = await db
		.select({ session_path: tasks.session_path })
		.from(tasks)
		.where(eq(tasks.id, taskId))
		.limit(1);
	const sp = rows[0]?.session_path;
	const basePath = sp ? resolve(REPO_ROOT, sp) : join(WORKFOLDER, taskId);

	if (type === 'video') {
		const videoPath = join(basePath, 'media', 'video_source.mp4');
		if (!existsSync(videoPath)) {
			console.log(JSON.stringify({ ok: false, error: 'video_source.mp4 not found' }));
			process.exit(1);
		}
		const stat = statSync(videoPath);
		console.log(
			JSON.stringify({
				ok: true,
				type: 'video',
				path: videoPath,
				size: stat.size,
			}),
		);
		return;
	}

	if (type === 'asr') {
		const asrPath = join(basePath, 'metadata', 'asr_fixed.json');
		const asrRawPath = join(basePath, 'metadata', 'asr.json');
		let asrFile = asrPath;
		if (!existsSync(asrPath) && existsSync(asrRawPath)) asrFile = asrRawPath;
		if (!existsSync(asrFile)) {
			console.log(JSON.stringify({ ok: false, error: 'asr.json not found' }));
			process.exit(1);
		}
		const asr = JSON.parse(readFileSync(asrFile, 'utf-8'));
		const segments = asr.result?.segments ?? [];
		const audioDurationMs = asr.audio_info?.duration ?? 0;
		const total = segments.length;
		const timeline: Record<string, unknown>[] = [];
		const issues: Record<string, unknown>[] = [];
		let zeroGaps = 0;

		for (let i = 0; i < total; i++) {
			const s = segments[i] as Record<string, any>;
			const entry: Record<string, unknown> = {
				idx: i + 1,
				text: (s.text ?? '').slice(0, 60),
				startMs: Math.round(s.start * 1000),
				endMs: Math.round(s.end * 1000),
			};
			const gapMs =
				i > 0
					? Math.round((s.start - (segments[i - 1] as Record<string, any>).end) * 1000)
					: 0;
			entry.gapMs = gapMs;
			if (gapMs === 0) zeroGaps++;
			const warnings: string[] = [];
			if (i > 0 && gapMs === 0) warnings.push('start 紧跟上段结束');
			if (
				Math.round(s.end * 1000) === audioDurationMs ||
				(i < total - 1 &&
					Math.round(s.end * 1000) === Math.round((segments[i + 1] as Record<string, any>).start * 1000))
			)
				warnings.push('end 拉到分段边界');
			const durationMs = Math.round((s.end - s.start) * 1000);
			if (durationMs > 5000)
				warnings.push(`时长 ${(durationMs / 1000).toFixed(1)}s 超过 5s`);
			if (warnings.length > 0) entry.warnings = warnings;
			timeline.push(entry);
		}

		if (total > 1 && zeroGaps === total - 1) {
			issues.push({
				type: 'vad_not_fired',
				detail: `全部 ${total - 1} 个段间间隙为 0ms，VAD 可能未生效`,
				suggestion: '检查 ASR 引擎的 VAD 配置',
			});
		} else if (zeroGaps > 0) {
			issues.push({
				type: 'partial_zero_gaps',
				detail: `${zeroGaps}/${total - 1} 个段间间隙为 0ms`,
			});
		}

		const engine =
			asr._device != null
				? asr._device === 'cpu'
					? 'faster-whisper'
					: 'whisper-pytorch'
				: 'unknown';
		const result: Record<string, unknown> = {
			ok: true,
			type: 'asr',
			engine,
			audioDurationMs,
			segments: total,
			zeroGaps,
			timeline,
		};
		if (issues.length > 0) result.issues = issues;
		console.log(JSON.stringify(result, null, 2));
		return;
	}

	console.log(JSON.stringify({ ok: false, error: `unknown check type: ${type}` }));
	process.exit(1);
}
