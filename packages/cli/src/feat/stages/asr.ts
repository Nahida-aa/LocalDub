import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import type { MLDaemon } from '../../ml/daemon/client.ts';
import {
	pythonBin,
	REPO_ROOT,
	readConfig,
	setLocalInfo,
} from '../config/config.ts';
import { emitLog, ffmpeg, nowISO, readTaskLanguages, updateStageDB } from './utils.ts';

export type AsrResult = {
	audio_info: {
		duration: number; // 视频总时长，单位 ms
	};
	result: {
		text: string; // 完整转录文本
		segments: {
			text: string; // 该段文本
			start: number; // 该段开始时间，单位 s
			end: number; // 该段结束时间，单位 s
			words?: [];
		}[];
	};
	_device: string; // 运行设备，如 "cuda"、"cpu" 等
	detected_language?: string; // 可选的检测到的语言代码，如 "en"、"zh" 等
};

export async function stageAsr(
	taskId: string,
	sessionPath: string,
	daemon?: MLDaemon,
) {
	await updateStageDB(taskId, 'asr', {
		last_message: 'Transcribing...',
		progress: 0,
	});
	const sessionAbsPath = resolve(REPO_ROOT, sessionPath);
	const audioVocal = resolve(sessionAbsPath, 'media', 'audio_vocals.wav');
	const videoSource = resolve(sessionAbsPath, 'media', 'video_source.mp4');

	let audioPath = readConfig().stages?.asr?.useSeparated
		? audioVocal
		: videoSource;
	if (!existsSync(audioPath))
		throw new Error(
			`ASR input not found: ${audioPath}; 如果 asr.useSeparated=true, 请确保 audio_vocals.wav 存在；如果 asr.useSeparated=false, 请确保 video_source.mp4 存在`,
		);

	if (readConfig().stages?.asr?.useSeparated) {
		const gatedVocal = resolve(sessionAbsPath, 'media', 'audio_vocals_gated.wav');
		emitLog(taskId, '[ASR] Applying silence gate to Demucs vocals...');
		ffmpeg([
			'-i', audioVocal,
			'-af', 'agate=threshold=0.02:ratio=20:attack=10:release=100',
			'-y', gatedVocal,
		]);
		audioPath = gatedVocal;
	}

	const asrCfg = readConfig().stages?.asr;
	const runtime = asrCfg?.runtime ?? 'pytorch';
	const device = asrCfg?.device ?? 'cuda';
	emitLog(taskId, `[ASR] runtime=${runtime} device=${device}`);

	const pyBin = pythonBin();
	const { asrLanguage } = readTaskLanguages(sessionPath);

	if (runtime === 'pytorch' && daemon?.ready) {
		emitLog(taskId, `[ASR] Using Python daemon (device=${device})`);
		const result = await daemon.runStage('asr', taskId, {
			vocals_path: audioPath,
			session_path: sessionAbsPath,
			language: asrLanguage || 'auto',
			device,
		});
		const r = result as Record<string, any>;
		const actualDevice: string = r.actual_device ?? device;
		const fallbackToCpu = device !== 'cpu' && actualDevice === 'cpu';
		if (fallbackToCpu) {
			console.warn(
				`[WARN] [ASR] GPU failed, fell back to CPU (actual device: ${actualDevice})`,
			);
		}
		if (r.detected_language) {
			setLocalInfo(sessionAbsPath, {
				asr_language: r.detected_language,
				runInfo: {
					asr: {
						engine: 'whisper-pytorch',
						device: actualDevice,
						gpuAttempted: device !== 'cpu',
						fallbackToCpu,
					},
				},
			});
		}
		if (r.load_time_s)
			emitLog(taskId, `[ASR] Model loaded in ${r.load_time_s}s`);
		if (r.process_time_s)
			emitLog(taskId, `[ASR] Transcribed in ${r.process_time_s}s`);
		if (r.audio_duration_s)
			emitLog(
				taskId,
				`[ASR] Audio duration ${Number(r.audio_duration_s).toFixed(1)}s`,
			);
		if (r.rtf) emitLog(taskId, `[ASR] RTF ${r.rtf}`);
	} else if (runtime === 'pytorch') {
		await asrPytorch(
			taskId,
			audioPath,
			sessionAbsPath,
			asrLanguage,
			device,
			pyBin,
		);
	} else {
		await asrFasterWhisper(
			taskId,
			audioPath,
			sessionAbsPath,
			asrLanguage,
			device,
			pyBin,
		);
	}

	// 统一过滤超出音频时长的幻觉段（所有路径 shared）
	const metadataDir = resolve(sessionAbsPath, 'metadata');
	const asrFile = resolve(metadataDir, 'asr.json');
	if (existsSync(asrFile)) {
		const data = JSON.parse(readFileSync(asrFile, 'utf-8'));
		const durationMs = data.audio_info?.duration ?? 0;
		if (durationMs > 0 && data.result?.segments?.length) {
			const before = data.result.segments.length;
			data.result.segments = data.result.segments.filter(
				(u: Record<string, any>) => u.start * 1000 < durationMs && u.end * 1000 > 0,
			);
			if (data.result.segments.length < before) {
				const removed = before - data.result.segments.length;
				emitLog(taskId, `[ASR] Removed ${removed} hallucinated segment(s) (start >= ${durationMs}ms or end <= 0ms)`);
				writeFileSync(asrFile, JSON.stringify(data, null, 2));
			}
		}
	}

	await updateStageDB(taskId, 'asr', {
		status: 'succeeded',
		completed_at: nowISO(),
		progress: 100,
		last_message: 'Transcribed',
	});
}

async function asrPytorch(
	taskId: string,
	audioPath: string,
	sessionAbsPath: string,
	language: string | undefined,
	device: string,
	pyBin: string,
) {
	const script = join(
		REPO_ROOT,
		'packages',
		'cli',
		'scripts',
		'asr',
		'pytorch.py',
	);

	if (device === 'cuda') {
		const cudaResult = spawnSync(pyBin, [
			'-c',
			'import torch; print(torch.cuda.is_available())',
		]);
		const cudaOk =
			cudaResult.status === 0 &&
			(cudaResult.stdout?.toString().trim() ?? '') === 'True';
		if (!cudaOk) {
			console.warn(
				`[WARN] [ASR] torch.cuda.is_available()=${cudaOk}, falling back to CPU`,
			);
			device = 'cpu';
		}
	}

	const attempts = device !== 'cpu' ? 2 : 1;
	let fallbackToCpu = false;

	for (let attempt = 0; attempt < attempts; attempt++) {
		const actualDevice = attempt === 0 ? device : 'cpu';
		const args = [
			script,
			audioPath,
			sessionAbsPath,
			language || 'auto',
			'--device',
			actualDevice,
		];
		const t0 = Date.now();
		const result = spawnSync(pyBin, args, {
			maxBuffer: 256 * 1024 * 1024,
			timeout: 600_000,
		});
		const elapsedSec = (Date.now() - t0) / 1000;

		if (result.error || result.signal || result.status !== 0) {
			if (attempt === 0 && device !== 'cpu') {
				const stderr = (result.stderr?.toString() || '').trim().slice(-200);
				console.warn(
					`[WARN] [ASR] GPU failed (${result.error?.message || `signal ${result.signal}` || `exit ${result.status}`}), retrying CPU: ${stderr}`,
				);
				await updateStageDB(taskId, 'asr', {
					last_message: 'GPU failed, retrying CPU...',
				});
				fallbackToCpu = true;
				continue;
			}
			if (result.error)
				throw new Error(`Python ASR subprocess failed: ${result.error.message}`);
			if (result.signal)
				throw new Error(
					`ASR killed by signal ${result.signal}: ${(result.stderr?.toString() || '').trim().slice(-200)}`,
				);
			throw new Error(
				`Python ASR exited with status ${result.status}: ${result.stderr?.toString() || ''}`,
			);
		}

		const asrOutputPath = parseAsrOutput(result.stdout?.toString() || '');
		if (!asrOutputPath || !existsSync(asrOutputPath)) {
			throw new Error(`Python ASR did not produce output at ${asrOutputPath}`);
		}

		const asr = JSON.parse(readFileSync(asrOutputPath, 'utf-8'));
		if (asr.detected_language) {
			setLocalInfo(sessionAbsPath, {
				asr_language: asr.detected_language,
				runInfo: {
					asr: {
						engine: 'whisper-pytorch',
						device: actualDevice,
						gpuAttempted: device !== 'cpu',
						fallbackToCpu,
					},
				},
			});
		}
		emitAsrTiming(taskId, asr, elapsedSec);
		return;
	}
}

function parseAsrOutput(stdout: string): string | null {
	for (const line of stdout.split('\n')) {
		const trimmed = line.trim();
		if (trimmed.startsWith('ASR_OUTPUT:'))
			return trimmed.slice('ASR_OUTPUT:'.length).trim();
	}
	return stdout.trim() || null;
}

function emitAsrTiming(taskId: string, asr: Record<string, any>, elapsedSec: number) {
	emitLog(taskId, `[ASR] Transcribed in ${elapsedSec.toFixed(1)}s`);
	const durationMs = asr.audio_info?.duration ?? 0;
	if (durationMs > 0) {
		const audioDurationS = durationMs / 1000;
		emitLog(taskId, `[ASR] Audio duration ${audioDurationS.toFixed(1)}s`);
		emitLog(taskId, `[ASR] RTF ${(elapsedSec / audioDurationS).toFixed(2)}`);
	}
}

async function asrFasterWhisper(
	taskId: string,
	audioPath: string,
	sessionAbsPath: string,
	language: string | undefined,
	device: string,
	pyBin: string,
) {
	const asrScript = join(
		REPO_ROOT,
		'packages',
		'cli',
		'scripts',
		'asr',
		'run.py',
	);
	const baseArgs = [asrScript, audioPath, sessionAbsPath, language || 'auto'];

	const useGpu = device !== 'cpu';
	const attempts = useGpu ? 2 : 1;
	let fallbackToCpu = false;

	for (let attempt = 0; attempt < attempts; attempt++) {
		const args = attempt === 0 && useGpu ? baseArgs : [...baseArgs, '--cpu'];
		const t0 = Date.now();
		const result = spawnSync(pyBin, args, {
			maxBuffer: 256 * 1024 * 1024,
			timeout: 600_000,
		});
		const elapsedSec = (Date.now() - t0) / 1000;

		if (result.signal) {
			const stderr = (result.stderr?.toString() || '').trim().slice(-200);
			if (attempt === 0 && useGpu) {
				await updateStageDB(taskId, 'asr', {
					last_message: 'GPU hang, retrying CPU...',
				});
				fallbackToCpu = true;
				continue;
			}
			throw new Error(`ASR killed by signal ${result.signal}: ${stderr}`);
		}

		if (result.error)
			throw new Error(`Python ASR subprocess failed: ${result.error.message}`);
		if (result.status !== 0) {
			const stderr = result.stderr?.toString() || '';
			throw new Error(
				`Python ASR exited with status ${result.status}: ${stderr}`,
			);
		}

		const asrOutputPath = parseAsrOutput(result.stdout?.toString() || '');
		if (!asrOutputPath || !existsSync(asrOutputPath)) {
			throw new Error(`Python ASR did not produce output at ${asrOutputPath}`);
		}

		const asr = JSON.parse(readFileSync(asrOutputPath, 'utf-8'));
		const actualDevice = fallbackToCpu ? 'cpu' : useGpu ? 'cuda' : 'cpu';
		if (asr.detected_language) {
			setLocalInfo(sessionAbsPath, {
				asr_language: asr.detected_language,
				runInfo: {
					asr: {
						engine: 'faster-whisper',
						device: actualDevice,
						computeType: actualDevice === 'cpu' ? 'int8' : 'float16',
						gpuAttempted: useGpu,
						fallbackToCpu,
					},
				},
			});
		}
		emitAsrTiming(taskId, asr, elapsedSec);

		return;
	}
}
