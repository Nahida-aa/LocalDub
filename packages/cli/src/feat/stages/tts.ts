import { spawn } from 'node:child_process';
import { readJson, writeFile, ensureDir, removeFile } from './fileOps.ts';
import { existsSync, readdirSync, rmSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import {
	VoxCPMCloud,
	VoxCPMNodeONNX,
	VoxCPMPython,
	writeWav,
} from '@repo/voxlab';
import type { MLDaemon } from '../../ml/daemon/client.ts';
import { pythonBin, REPO_ROOT, readConfig } from '../config/config.ts';
import type { Device, TTSConfig } from '../config/types.ts';
import { emitLog, nowISO, readTaskLanguages, updateStageDB } from './utils/utils.ts';
import { TranslateFile } from './translate.ts';

function createTTSBackend(cfg: TTSConfig) {
	if (!cfg) throw new Error('TTS config not found');
	if (cfg.runtime === 'cloud') return new VoxCPMCloud();
	if (cfg.runtime === 'pytorch') return new VoxCPMPython();
	const device =
		'device' in cfg ? (cfg.device === 'webgpu' ? 'webgpu' : 'cpu') : 'cpu';
	return new VoxCPMNodeONNX({ executionProvider: device });
}

async function runPytorchBatch(
	taskId: string,
	ttsCfg: NonNullable<TTSConfig>,
	translationFile: string,
	vocalsDir: string,
	ttsDir: string,
	total: number,
) {
	if (!ttsCfg) throw new Error('TTS config not found');

	const device = 'device' in ttsCfg ? (ttsCfg.device as Device) : 'cuda';
	const scriptPath = join(
		REPO_ROOT,
		'packages',
		'voxlab',
		'scripts',
		'voxcpm_infer_batch.py',
	);
	const modelDir = join(REPO_ROOT, 'data', 'modelscope', 'OpenBMB__VoxCPM2');
	const pyBin = pythonBin();
	const voxcpmSrc = join(REPO_ROOT, 'submodule', 'VoxCPM', 'src');

	return new Promise<void>((resolve, reject) => {
		const args = [
			scriptPath,
			'--model-dir', modelDir,
			'--translation-file', translationFile,
			'--vocals-dir', vocalsDir,
			'--tts-dir', ttsDir,
			'--device', device,
		];
		if (ttsCfg.skipExisting) args.push('--skip-existing');
		const proc = spawn(pyBin, args,
			{
				env: { ...process.env, PYTHONPATH: voxcpmSrc },
			},
		);

		let stderr = '';

		proc.stdout.on('data', (chunk: Buffer) => {
			const lines = chunk.toString().split('\n').filter(Boolean);
			for (const line of lines) {
				const progressMatch = line.match(/^\[PROGRESS\] (\d+)\/(\d+)$/);
				if (progressMatch) {
					const current = parseInt(progressMatch[1]);
					const ttl = parseInt(progressMatch[2]);
					updateStageDB(taskId, 'tts', {
						last_message: `Generating ${current}/${ttl}...`,
					});
				} else if (line.startsWith('{')) {
					try {
						const result = JSON.parse(line);
						emitLog(
							taskId,
							`[TTS] Batch complete: ${result.generated} generated, ${result.skipped} skipped, ${result.errors} errors in ${result.total_time_s}s`,
						);
						if (result.generate_time_s) {
							emitLog(
								taskId,
								`[VoxCPM] Generated in ${result.total_time_s}s | RTF ${result.rtf}`,
							);
						}
					} catch {
						/* not JSON */
					}
				}
			}
		});

		proc.stderr.on('data', (chunk: Buffer) => {
			stderr += chunk.toString();
		});

		proc.on('close', (code) => {
			if (code !== 0) {
				const errMsg = stderr.slice(-500);
				reject(new Error(`Python batch TTS exit code ${code}: ${errMsg}`));
				return;
			}
			resolve();
		});

		proc.on('error', reject);
	});
}

export async function stageTts(
	taskId: string,
	sessionPath: string,
	daemon?: MLDaemon,
) {
	const ttsCfg = readConfig().stages?.tts!
	const { targetLanguage: dstLangCode } = readTaskLanguages(sessionPath);
	const translationFile = resolve(
		REPO_ROOT,
		sessionPath,
		'metadata',
		`translation.${dstLangCode}.json`,
	);
	const vocalsDir = resolve(REPO_ROOT, sessionPath, 'segments', 'vocals');
	const ttsDir = resolve(REPO_ROOT, sessionPath, 'segments', 'tts');

	if (!existsSync(translationFile))
		throw new Error(`${translationFile} not found`);
	ensureDir(ttsDir, 'TTS', taskId);

	const data: TranslateFile = readJson(translationFile, 'TTS', taskId);
	const translation = data.translation;

	const anyTts = readdirSync(ttsDir).find((f) => f.endsWith('.wav'));
	if (
		anyTts &&
		statSync(translationFile).mtimeMs > statSync(join(ttsDir, anyTts)).mtimeMs
	) {
		for (const f of readdirSync(ttsDir)) rmSync(join(ttsDir, f));
		const sessionAbs = resolve(REPO_ROOT, sessionPath);
		const dubbingFile = join(sessionAbs, 'tmp', 'audio_dubbing.wav');
		const finalVideo = join(sessionAbs, 'media', `${taskId}_dub.mp4`);
		for (const f of [dubbingFile, finalVideo]) {
			if (existsSync(f)) rmSync(f);
		}
	}

	if (ttsCfg.runtime === 'pytorch' && daemon?.ready) {
		emitLog(taskId, `[TTS] Using Python daemon (device=${ttsCfg.device})`);
		const modelDir = join(REPO_ROOT, 'data', 'modelscope', 'OpenBMB__VoxCPM2');
		const result = await daemon.runStage(
			'tts',
			taskId,
			{
				translation_file: translationFile,
				vocals_dir: vocalsDir,
				tts_dir: ttsDir,
				model_dir: modelDir,
				device: ttsCfg.device,
				skipExisting: ttsCfg.skipExisting,
			},
			(current, total) => {
				emitLog(taskId, `[TTS] ${current}/${total}`);
				updateStageDB(taskId, 'tts', {
					last_message: `Generating ${current}/${total}...`,
				});
			},
		);
		const r = result as Record<string, number>;
		emitLog(
			taskId,
			`[TTS] Batch complete: ${r.generated ?? 0} generated, ${r.skipped ?? 0} skipped, ${r.errors ?? 0} errors`,
		);
		if (r.load_time_s)
			emitLog(taskId, `[TTS] Model loaded in ${r.load_time_s}s`);
		if (r.generate_time_s)
			emitLog(taskId, `[TTS] Generated in ${r.generate_time_s}s`);
		if (r.total_time_s)
			emitLog(taskId, `[TTS] Total ${r.total_time_s}s (load + generate)`);
		if (r.rtf != null)
			emitLog(taskId, `[VoxCPM] RTF ${r.rtf}`);
		if (r.errors && r.errors > 0)
			emitLog(taskId, `[WARN] [TTS] ${r.errors} segments had errors`);
		await updateStageDB(taskId, 'tts', {
			status: 'succeeded',
			completed_at: nowISO(),
			progress: 100,
			last_message: 'TTS done',
		});
		return;
	}

	if (ttsCfg.runtime === 'pytorch') {
		await runPytorchBatch(
			taskId,
			ttsCfg,
			translationFile,
			vocalsDir,
			ttsDir,
			translation.length,
		);
	} else {
		let fallbackRef = '';
		for (let i = 0; i < translation.length; i++) {
			const idx = String(i + 1).padStart(4, '0');
			const refPath = resolve(vocalsDir, `${idx}.wav`);
			if (existsSync(refPath) && statSync(refPath).size > 1200 * 16 * 2) {
				fallbackRef = refPath;
				break;
			}
		}

		const voxcpm = createTTSBackend(ttsCfg);
		await voxcpm.load();

		const t0 = performance.now();
		let genMs = 0;
		for (let i = 0; i < translation.length; i++) {
			const item = translation[i];
			const idx = String(i + 1).padStart(4, '0');
			const outPath = resolve(ttsDir, `${idx}.wav`);
			if (ttsCfg.skipExisting && existsSync(outPath)) continue;

			const text = item.dst || '';
			if (!text.trim()) {
				writeFile(outPath, Buffer.alloc(44), 'TTS', taskId);
				continue;
			}

			let refWav = resolve(vocalsDir, `${idx}.wav`);
			if (!existsSync(refWav) || statSync(refWav).size < 1200 * 16 * 2) {
				refWav = fallbackRef;
			}
			if (!refWav || !existsSync(refWav)) {
				emitLog(
					taskId,
					`[WARN] [TTS] No reference for segment ${idx}, skipping`,
				);
				writeFile(outPath, Buffer.alloc(44), 'TTS', taskId);
				continue;
			}

			await updateStageDB(taskId, 'tts', {
				last_message: `Generating ${i + 1}/${translation.length}...`,
			});

			const t1 = performance.now();
			const { samples: audio } = await voxcpm.generate({
				text,
				referenceWavPath: refWav,
				promptText: item.src,
			});
			genMs += performance.now() - t1;
			writeWav(audio, outPath, 48000);
		}

		await voxcpm.dispose();

		const genSec = genMs / 1000;
		const audioSec = translation.reduce((s, t) => s + (t.end_time - t.start_time), 0) / 1000;
		const rtf = audioSec > 0 && genSec > 0 ? genSec / audioSec : 0;
		emitLog(taskId, `[VoxCPM] Generated in ${genSec.toFixed(1)}s | RTF ${rtf.toFixed(3)}`);
	}

	await updateStageDB(taskId, 'tts', {
		status: 'succeeded',
		completed_at: nowISO(),
		progress: 100,
		last_message: 'TTS done',
	});
}
