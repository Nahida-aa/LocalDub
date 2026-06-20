import { spawnSync } from 'node:child_process';
import {
	existsSync,
	mkdirSync,
	readFileSync,
	statSync,
	writeFileSync,
} from 'node:fs';
import { basename, join, relative } from 'node:path';
import { env, REPO_ROOT, WORKFOLDER, YOUTUBE_COOKIE_PATH } from '@repo/config';
import { sanitizeText } from './../../feat/tasks/fn.ts';
import { extractVideoId, isYouTubeUrl } from './../../feat/tasks/validate.ts';
import {
	emitLog,
	ffmpeg,
	nowISO,

} from './utils/utils.ts';
import { Context, readCtx, setCtx, setStage, setTask, writeCtx } from '../context/context.ts';
import { to } from '@repo/shared/lib/utils/try.ts';

export async function stageDownload(
	ctx: Context,
) {
	const taskId = ctx.task.id;
	const url = ctx.task.url;

	// Local upload URL (local://upload/<uploadTaskId>)
	if (ctx.task.source === 'local') {
		const sessionPath = ctx.task.session_path;
		let mediaDir = join(sessionPath, 'media');
		let videoPath = join(mediaDir, 'video_source.mp4');
		emitLog(sessionPath, '[Download] Importing local video...');
		await setStage(sessionPath, 'download', {
			last_message: 'Importing local video...',
			progress: 0,
		});
		mkdirSync(mediaDir, { recursive: true });
		mkdirSync(join(sessionPath, 'metadata'), { recursive: true });

		const t0 = Date.now();
		ffmpeg([
			'-i',
			ctx.task.url,
			'-map',
			'0:v:0',
			'-map',
			'0:a:0?',
			'-c:v',
			'libx264',
			'-preset',
			'fast',
			'-crf',
			'23',
			'-c:a',
			'aac',
			'-movflags',
			'+faststart',
			videoPath,
		]);
		const elapsedSec = (Date.now() - t0) / 1000;

		if (!existsSync(videoPath))
			throw new Error('ffmpeg did not produce video_source.mp4');

		const sizeMb = (statSync(videoPath).size / 1024 / 1024).toFixed(1);
		emitLog(sessionPath, `[Download] Imported in ${elapsedSec.toFixed(1)}s (${sizeMb}MB)`);

		// Preserve existing fields  from fn.ts
		const existing = readCtx(sessionPath);
		writeCtx( {
			...existing,
			task: {
				...existing.task,
				title: basename(ctx.task.url).replace(/\.\w+$/, ''),
			},
		});

		await setStage(sessionPath, 'download', {
			status: 'succeeded',
			completed_at: nowISO(),
			progress: 100,
			last_message: 'Imported',
		});
		return;
	} else if (ctx.task.source === 'youtube' || ctx.task.source === 'bilibili') {
		// YouTube/Bilibili URL — download via yt-dlp

		const isYT = ctx.task.source === 'youtube'
		const authArgs: string[] = [];
		if (isYT && existsSync(YOUTUBE_COOKIE_PATH))
			authArgs.push('--cookies', YOUTUBE_COOKIE_PATH);
		if (isYT && env.YTDLP_PROXY_PORT)
			authArgs.push('--proxy', `http://127.0.0.1:${env.YTDLP_PROXY_PORT}`);

		let sessionPath = '';
		try {
			const infoArgs = ['--dump-json', ...authArgs, url];
			const infoR = spawnSync('yt-dlp', infoArgs, {
				stdio: ['pipe', 'pipe', 'pipe'],
				timeout: 30_000,
			});
			if (infoR.status === 0 && infoR.stdout.length > 0) {
				const info = JSON.parse(infoR.stdout.toString());
				const uploader = sanitizeText(info.uploader || '', 'unknown');
				const title = sanitizeText(info.title || '', 'untitled');
				const videoId = info.id || extractVideoId(url);
				sessionPath = join(WORKFOLDER, uploader, `${title}__${videoId}`);

				mkdirSync(join(sessionPath, 'metadata'), { recursive: true });
				writeFileSync(
					join(sessionPath, 'metadata', 'ytdlp_info.json'),
					infoR.stdout,
				);
				await setTask(sessionPath, {
					session_path: relative(REPO_ROOT, sessionPath),
				});
			}
		} catch {
			/* fall back to flat path */
		}

		const mediaDir = join(sessionPath, 'media');
		const videoPath = join(mediaDir, 'video_source.mp4');

		emitLog(sessionPath, '[Download] Downloading video...');
		await setStage(sessionPath, 'download', {
			last_message: 'Downloading video...',
			progress: 0,
		});
		mkdirSync(mediaDir, { recursive: true });
		mkdirSync(join(sessionPath, 'metadata'), { recursive: true });

		const ytArgs: string[] = [
			'-f',
			'bestaudio[ext=m4a]+bestvideo[ext=mp4]/best[ext=mp4]/best',
			'--merge-output-format',
			'mp4',
			'-o',
			join(mediaDir, 'video_source.%(ext)s'),
			...authArgs,
			url,
		];

		const t0 = Date.now();
		const r = spawnSync('yt-dlp', ytArgs, {
			stdio: ['pipe', 'pipe', 'pipe'],
			timeout: 300_000,
		});
		const elapsedSec = (Date.now() - t0) / 1000;

		const dlErr = r.error;
		if (dlErr) throw new Error(`yt-dlp: ${dlErr.message}`);
		if (r.status !== 0)
			throw new Error(
				`yt-dlp exit ${r.status}: ${r.stderr.toString().slice(0, 200)}`,
			);

		if (!existsSync(videoPath))
			throw new Error('yt-dlp did not produce video_source.mp4');

		const sizeMb = (statSync(videoPath).size / 1024 / 1024).toFixed(1);
		emitLog(sessionPath, `[Download] Downloaded in ${elapsedSec.toFixed(1)}s (${sizeMb}MB)`);
		emitLog(sessionPath, `[Download] Speed ${(Number(sizeMb) / elapsedSec).toFixed(2)} MB/s`);

		await setStage(sessionPath, 'download', {
			status: 'succeeded',
			completed_at: nowISO(),
			progress: 100,
			last_message: 'Downloaded',
		});
		await setCtx(sessionPath, {
			video_file_path: relative(REPO_ROOT, videoPath),
		});
	}
}
