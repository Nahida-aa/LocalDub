import { spawn, spawnSync } from 'node:child_process';
import { readJson, writeJson, ensureDir, removeFile } from './fileOps.ts';
import { existsSync } from 'node:fs';
import { join, resolve } from 'node:path';
import type { MLDaemon } from '../../ml/daemon/client.ts';
import {
	pythonBin,
	REPO_ROOT,
	readConfig,
	setLocalInfo,
} from '../config/config.ts';
import { emitLog, ffmpeg, nowISO, readTaskLanguages, srtTime, updateStageDB } from './utils/utils.ts';
import { AsrOptions } from './asr/types.ts';
import { parseAsrOutput } from './asr/utils.ts';



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
	const audioVocal = resolve(sessionAbsPath, 'media', 'target_3_vocals.wav');
	const videoSource = resolve(sessionAbsPath, 'media', 'video_source.mp4');

	let audioPath = readConfig().stages?.asr?.useSeparated
		? audioVocal
		: videoSource;
	if (!existsSync(audioPath))
		throw new Error(
			`ASR input not found: ${audioPath}; 如果 asr.useSeparated=true, 请确保 target_3_vocals.wav 存在；如果 asr.useSeparated=false, 请确保 video_source.mp4 存在`,
		);

	if (readConfig().stages?.asr?.useSeparated) {
		const asrCfg = readConfig().stages?.asr;
		const mixMode = asrCfg?.mixMode ?? 'vocals';
		const reduceBgm = asrCfg?.reduceBgm ?? -12;
		const sc = asrCfg?.sidechainCompress!
		console.log(`[ASR] mixMode=${mixMode} reduceBgm=${reduceBgm}dB sidechainCompress=${JSON.stringify(sc)}`);
		const useGate = asrCfg?.useGate ?? false;
		let sourcePath = audioVocal;
		if (mixMode === 'raw-sum') {
			const bgmPath = resolve(sessionAbsPath, 'media', 'target_bgm.wav');
			if (!existsSync(bgmPath)) {
				emitLog(taskId, `[ASR] target_bgm.wav not found, skipping BGM reduction`);
			} else {
				const mixedPath = resolve(sessionAbsPath, 'media', 'target_3_vocals_mixed.wav');
				emitLog(taskId, `[ASR] raw-sum: mixing vocals + BGM at ${reduceBgm}dB...`);
				ffmpeg([
					'-i', sourcePath,
					'-i', bgmPath,
					'-filter_complex',
					`[1:a]volume=${reduceBgm}dB[bgm_r];[0:a][bgm_r]amix=inputs=2:duration=first:weights=1 1[out]`,
					'-map', '[out]',
					'-y', mixedPath,
				]);
				sourcePath = mixedPath;
			}
		} else if (mixMode === 'sidechain') {
			const bgmPath = resolve(sessionAbsPath, 'media', 'target_bgm.wav');
			if (!existsSync(bgmPath)) {
				emitLog(taskId, `[ASR] target_bgm.wav not found, skipping BGM reduction`);
			} else {
				const mixedPath = resolve(sessionAbsPath, 'media', 'target_3_vocals_mixed.wav');
				const scParams = `threshold=${sc.threshold ?? 0.1}:ratio=${sc.ratio ?? 20}:attack=${sc.attack ?? 1}:release=${sc.release ?? 500}`;
				const bgmVol = reduceBgm !== 0 ? `[bgm_sc]volume=${reduceBgm}dB[bgm_final]` : null;
				emitLog(taskId, `[ASR] sidechain: ${scParams}, bgmReduce=${reduceBgm}dB`);
				ffmpeg([
					'-i', sourcePath,
					'-i', bgmPath,
					'-filter_complex',
					`[0:a]asplit[v][v_key];[1:a][v_key]sidechaincompress=${scParams}[bgm_sc]${bgmVol ? `;${bgmVol}` : ''};[v][${bgmVol ? 'bgm_final' : 'bgm_sc'}]amix=inputs=2:duration=first:weights=1 1[out]`,
					'-map', '[out]',
					'-y', mixedPath,
				]);
				sourcePath = mixedPath;
			}
		}

		if (useGate) {
			const gatedPath = resolve(sessionAbsPath, 'media', 'target_3_vocals_gated.wav');
			emitLog(taskId, '[ASR] Applying silence gate...');
			ffmpeg([
				'-i', sourcePath,
				'-af', 'agate=threshold=0.02:ratio=20:attack=10:release=100',
				'-y', gatedPath,
			]);
			sourcePath = gatedPath;
		}

		audioPath = sourcePath;
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
		await asrPytorch({ taskId, audioPath, sessionPath: sessionAbsPath, language: asrLanguage, device, pythonBin: pyBin });
	} else if (runtime === 'ggml') {
		await asrWhisperCpp(taskId, audioPath, sessionAbsPath, asrLanguage || 'auto');
	} else {
		await asrFasterWhisper({ taskId, audioPath, sessionPath: sessionAbsPath, language: asrLanguage, device, pythonBin: pyBin });
	}

	/**
	 * asr 后处理, 避免 asr_fix 拿到多余数据
	 */
	// 统一过滤超出音频时长的幻觉段（所有路径 shared）
	const metadataDir = resolve(sessionAbsPath, 'metadata');
	const asrFile = resolve(metadataDir, 'asr.json');
	if (existsSync(asrFile)) {
		const data = readJson(asrFile, 'ASR');
		const durationMs = data.audio_info?.duration ?? 0;
		if (durationMs > 0 && data.result?.segments?.length) {
			const before = data.result.segments.length;
			data.result.segments = data.result.segments.filter(
				(u: Record<string, any>) => u.start < durationMs && u.end > 0,
			);
			if (data.result.segments.length < before) {
				const removed = before - data.result.segments.length;
				emitLog(taskId, `[ASR] Removed ${removed} hallucinated segment(s) (start >= ${durationMs}ms or end <= 0ms)`);
				writeJson(asrFile, data, 'ASR');
			}
		}

		// 能量检测：最后一个 segment 如果 RMS 过低则判为幻觉
		const last = data.result?.segments?.[data.result.segments.length - 1];
		if (last && existsSync(audioPath)) {
			const rms = await segmentRms(audioPath, last.start, last.end);
			console.log(`[ASR] Last segment RMS: ${rms}`);
			if (rms > 0 && rms < 0.005) {
				const removed = data.result.segments.pop();
				emitLog(taskId, `[ASR] Removed low-energy hallucinated segment "${removed.text.slice(0, 30)}" (RMS=${rms.toFixed(5)})`);
				writeJson(asrFile, data, 'ASR');
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

async function asrPytorch(opts: AsrOptions) {
	let { taskId, audioPath, sessionPath: sessionAbsPath, language, device, pythonBin: pyBin } = opts;
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

		const asr = readJson(asrOutputPath, 'ASR');
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


function emitAsrTiming(taskId: string, asr: Record<string, any>, elapsedSec: number) {
	emitLog(taskId, `[ASR] Transcribed in ${elapsedSec.toFixed(1)}s`);
	const durationMs = asr.audio_info?.duration ?? 0;
	if (durationMs > 0) {
		const audioDurationS = durationMs / 1000;
		emitLog(taskId, `[ASR] Audio duration ${audioDurationS.toFixed(1)}s`);
		emitLog(taskId, `[ASR] RTF ${(elapsedSec / audioDurationS).toFixed(2)}`);
	}
}

function segmentRms(audioPath: string, startMs: number, endMs: number): Promise<number> {
	return new Promise((resolve, reject) => {
		const args = [
			'-y', '-i', audioPath,
			'-ss', String(startMs / 1000),
			'-to', String(endMs / 1000),
			'-af', 'astats=metadata=1:reset=1,ametadata=print:key=lavfi.astats.Overall.RMS_level:file=-',
			'-f', 'null', '-',
		];
		const proc = spawn('ffmpeg', args, { timeout: 30_000 });
		let stderr = '';
		proc.stderr.on('data', (chunk: Buffer) => { stderr += chunk.toString(); });
		proc.on('close', (code) => {
			if (code !== 0) return resolve(0);
			const m = stderr.match(/lavfi\.astats\.Overall\.RMS_level=([-\d.]+)/);
			if (!m) return resolve(0);
			const db = parseFloat(m[1]);
			// dB → linear: linear = 10^(dB/20)
			resolve(Math.pow(10, db / 20));
		});
		proc.on('error', () => resolve(0));
	});
}

async function asrWhisperCpp(
	taskId: string,
	audioPath: string,
	sessionAbsPath: string,
	language: string,
) {
	const whisperCli = join(
		REPO_ROOT, 'submodule', 'whisper.cpp', 'build', 'bin', 'whisper-vulkan',
	);
	const model = process.env.WHISPER_MODEL || join(
		process.env.HOME || '/root', '.cache', 'pywhispercpp', 'ggml-large-v3-turbo.bin',
	);

	emitLog(taskId, `[ASR] runtime=ggml binary=${whisperCli}`);

	// whisper-cli writes <audioPath>.json alongside input; use a copy in tmp to avoid clobber
	const audioDir = resolve(REPO_ROOT, sessionAbsPath, 'tmp');
	ensureDir(audioDir, 'ASR');
	const tmpAudio = join(audioDir, 'whisper-input.wav');

	// Copy/convert input to WAV
	if (audioPath.endsWith('.wav')) {
		spawnSync('cp', [audioPath, tmpAudio], { timeout: 10_000 });
	} else {
		spawnSync('ffmpeg', ['-y', '-i', audioPath, '-ac', '1', tmpAudio], {
			timeout: 30_000,
		});
	}
	if (!existsSync(tmpAudio)) {
		throw new Error(`ffmpeg failed to convert ${audioPath} to ${tmpAudio}`);
	}

	const t0 = performance.now();
	const result = spawnSync(whisperCli, [
		'-m', model, tmpAudio, '-l', language, '-t', '4', '-ojf',
	], {
		timeout: 600_000,
		env: {
			...process.env,
			LD_LIBRARY_PATH: [
				join(whisperCli, '..', '..', 'src'),
				join(whisperCli, '..', '..', 'ggml', 'src'),
				join(whisperCli, '..', '..', 'ggml', 'src', 'ggml-hip'),
				process.env.LD_LIBRARY_PATH || '',
			].filter(Boolean).join(':'),
		},
	});
	const elapsedSec = (performance.now() - t0) / 1000;

	if (result.status !== 0 && result.status !== null) {
		throw new Error(`whisper-cli failed (${result.status}): ${result.stderr?.toString().slice(-300)}`);
	}

	// Read the generated JSON
	const whisperJson = `${tmpAudio}.json`;
	if (!existsSync(whisperJson)) {
		throw new Error(`whisper-cli did not produce ${whisperJson}`);
	}

	const raw = readJson(whisperJson, 'ASR');
	const transcription: any[] = raw.transcription || [];

	const emitWords = readConfig().stages?.asr?.wordsOutput ?? true;

	const segments = transcription.map((s: any) => {
		const startMs = s.offsets?.from ?? 0;
		const endMs = s.offsets?.to ?? 0;
		const seg: Record<string, any> = {
			text: (s.text || '').trim(),
			start: startMs,
			end: endMs,
			start_fmt: srtTime(startMs),
			end_fmt: srtTime(endMs),
		};
		if (emitWords) {
			const words = (s.tokens || [])
				.filter((t: any) => {
					const txt = t.text?.trim();
					return txt && !txt.startsWith('[') && t.offsets?.from != null;
				})
				.map((t: any) => ({
					word: t.text.trim(),
					start: t.offsets.from ?? 0,
					end: t.offsets.to ?? 0,
					probability: t.p,
				}));
			seg.words = words;
			const probs = words.map((w: any) => w.probability).filter((p: number) => p >= 0);
			if (probs.length > 0) {
				seg.confidence = {
					avg: +(probs.reduce((a: number, b: number) => a + b, 0) / probs.length).toFixed(4),
					min: +Math.min(...probs).toFixed(4),
				};
			}
		}
		return seg;
	});
	const text = segments.map(s => s.text).join(' ');

	const metadataDir = resolve(sessionAbsPath, 'metadata');
	ensureDir(metadataDir, 'ASR');

	const lastEndMs = segments.length ? segments[segments.length - 1].end : 0;
	const asrOutput = {
		audio_info: { duration: lastEndMs },
		result: { text, segments },
		_engine: 'whisper.cpp',
		_device: 'vulkan',
		_rtf: elapsedSec > 0 && lastEndMs > 0
			? (elapsedSec / (lastEndMs / 1000)).toFixed(3)
			: '0',
	};
	writeJson(join(metadataDir, 'asr.json'), asrOutput, 'ASR');

	// Cleanup tmp audio and json
	removeFile(tmpAudio, 'ASR');
	removeFile(whisperJson, 'ASR');

	emitLog(taskId, `[ASR] Transcribed in ${elapsedSec.toFixed(1)}s`);
	if (segments.length > 0) {
		const audioDurationMs = segments[segments.length - 1].end;
		emitLog(taskId, `[ASR] Audio duration ${(audioDurationMs / 1000).toFixed(1)}s`);
		emitLog(taskId, `[ASR] RTF ${(elapsedSec / (audioDurationMs / 1000)).toFixed(3)}`);
	}
}

async function asrFasterWhisper(opts: AsrOptions) {
	const { taskId, audioPath, sessionPath: sessionAbsPath, language, device, pythonBin: pyBin } = opts;
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

		const asr = readJson(asrOutputPath, 'ASR');
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
