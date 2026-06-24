import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { main as evalOcrMain } from '../../ref/compute/eval-ocr.ts';
import { createSessions, ocrFrameWithSessions, releaseSessions } from '../../../subtitle-ocr/subtitle-node.ts';
import { FrameResult, Segment, mergeFrames } from '../../../cli/src/feat/stages/utils/ocrMerge.ts';
import { REPO_ROOT } from '@repo/config';


/** Find rapidocr model directory dynamically.
 *  Supports multiple venv layouts (Windows/Linux standard venv and custom layouts).
 *  Use OCR_MODELS_DIR env var to override.
 */
function findRapidOCRModelsDir(): string {
	if (process.env.OCR_MODELS_DIR && existsSync(process.env.OCR_MODELS_DIR)) {
		return process.env.OCR_MODELS_DIR;
	}
	const venvBase = resolve(REPO_ROOT, '.venv');
	const modelsSub = join('rapidocr_onnxruntime', 'models');
	const candidates: string[] = [];
	if (process.platform === 'win32') {
		candidates.push(resolve(venvBase, 'Lib', 'site-packages', modelsSub));
	} else {
		const libDir = resolve(venvBase, 'lib');
		if (existsSync(libDir)) {
			const entries = readdirSync(libDir, { withFileTypes: true });
			const pyDirs = entries.filter(e =>
				e.isDirectory() && /^python\d+\.\d+$/.test(e.name)
			).map(e => e.name).sort().reverse();
			for (const pyDir of pyDirs) {
				candidates.push(resolve(libDir, pyDir, 'site-packages', modelsSub));
			}
			const sitePackagesLegacy = resolve(libDir, 'site-packages');
			if (existsSync(sitePackagesLegacy)) {
				const spEntries = readdirSync(sitePackagesLegacy, { withFileTypes: true });
				const spPyDirs = spEntries.filter(e =>
					e.isDirectory() && /^python\d+\.\d+$/.test(e.name)
				).map(e => e.name).sort().reverse();
				for (const pyDir of spPyDirs) {
					candidates.push(resolve(sitePackagesLegacy, pyDir, modelsSub));
				}
				candidates.push(resolve(sitePackagesLegacy, modelsSub));
			}
		}
	}
	for (const c of candidates) {
		if (existsSync(c)) return c;
	}
	throw new Error(
		`rapidocr models not found. Searched: ${candidates.join(', ')}\n` +
		`Hint: pip install rapidocr-onnxruntime in .venv, or set OCR_MODELS_DIR env var.`
	);
}

const VIDEOS_PATH = join(REPO_ROOT, 'packages', 'benchmark', 'ref', 'media');
const VIDEO_PATH = join(VIDEOS_PATH, 'video_source.mp4');
// 系统 PATH 可能包含极简版 ffmpeg（无 mjpeg 编码器），优先使用系统完整 ffmpeg
function findFullBin(name: string): string {
	const candidates = [`/usr/bin/${name}`, `/usr/local/bin/${name}`];
	for (const c of candidates) if (existsSync(c)) return c;
	return name;
}
const FFMPEG_BIN = process.env.BENCHMARK_FFMPEG || findFullBin('ffmpeg');
const FFPROBE_BIN = process.env.BENCHMARK_FFPROBE || findFullBin('ffprobe');
const CPP_BIN = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ort-cpp', 'build', 'subtitle_ocr_ort_cpp');
const CPP_LD_PATH = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ort-cpp', 'build');
const CPP_OPENCV_BIN = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ort-cpp', 'build', 'subtitle_ocr_ort_cpp');
const CPP_OPENCV_LD_PATH = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ort-cpp', 'build');
const RUST_BIN = resolve(REPO_ROOT, 'packages', 'subtitle-rust', 'target', 'release', 'ocr_pipeline_rs');
const RUST_INFER_PY = resolve(REPO_ROOT, 'packages', 'subtitle-rust', 'infer_onnx.py');
const OCR_MODELS_DIR = findRapidOCRModelsDir();
const OCR_KEYS_PATH = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ppocr_keys.json');
const PYTHON_BIN = join(REPO_ROOT, '.venv', 'bin', 'python');
const OCR_PY = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'subtitle-py.py');
const GROUND_TRUTH = resolve(REPO_ROOT, 'packages', 'benchmark', 'ref', 'metadata', 'ocr_manual.json');
const RESULTS_BASE = resolve(__dirname, '..', 'results');
const TMP = resolve(REPO_ROOT, 'packages', 'tmp', 'ocr-bench');
let globalLabelSuffix: string | null = null;

interface OCRLine {
	text: string;
	confidence: number;
}

interface OCRResult {
	lines: OCRLine[];
	inferenceMs: number;
}

function runEvalOCR(ocrPath: string, label: string): { wer: number; cer: number; hyp_chars: number; ref_chars: number; } {
	const result = evalOcrMain({ gtPath: GROUND_TRUTH, hypPath: ocrPath, label, hypScaleMs: 1 });
	return {
		wer: result.normalized.cer / 100,
		cer: result.normalized.cer / 100,
		hyp_chars: result.normalized.hypChars,
		ref_chars: result.normalized.refChars,
	};
}

function framesDir(label: string): string {
	return join(TMP, `frames-${label}`);
}

// 公共工具：取最高 confidence 的 segment（或 line）
function selectBest<T extends { confidence: number; text: string }>(
	segs: T[],
): { text: string; confidence: number } {
	if (!segs || segs.length === 0) return { text: '', confidence: 0 };
	return segs.reduce((a, b) => (a.confidence > b.confidence ? a : b));
}

// listFrameFiles 在每个函数里都重复写一次——统一在这里
function listFrameFiles(frameDir: string): string[] {
	return spawnSync('ls', [frameDir], { encoding: 'utf-8' })
		.stdout.trim().split('\n')
		.filter(f => f.endsWith('.jpg'))
		.sort();
}

interface Timings {
	total: number;
	averagePerFrame: number;
	det: number;
	post: number;
	rec: number;
}

interface FrameResultExt extends FrameResult {
	totalMs?: number;
	detMs?: number;
	postMs?: number;
	recMs?: number;
}

interface BenchmarkSummary {
	label: string;
	fps: number;
	engine: string;
	frames: number;
	segments: number;
	audio_duration_s: number;
	ocr_inference_s?: number;
	ocr_rtf?: number;
	ocr_timings_ms?: Timings;
	wer: number;
	cer: number;
	hyp_chars: number;
	ref_chars: number;
	textScore: number;
	subtitleOnly: boolean;
}

// runBenchmarkCommon — 把 5 个 benchmark 函数里重复 60-80 行相同的模板抽出来。
// 唯一差异是 "怎么跑 OCR"——交由回调 produceFrameResults 完成，它返回每帧的 text/confidence 和耗时。
// 可选 beforeOcr 用于引擎专属 setup（例如 Node 开 session、Rust 检查 bin）。
async function runBenchmarkCommon(
	label: string,
	fps: number,
	engine: string,
	opts: { textScore?: number; subtitleOnly?: boolean },
	produceFrameResults: (ctx: {
		frameFiles: string[];
		frameDir: string;
		step: number;
		srcFps: number;
	}) => FrameResultExt[] | Promise<FrameResultExt[]>,
	extraEngineField: Record<string, unknown> = {},
): Promise<any> {
	const { textScore = 0.45, subtitleOnly = false } = opts;
	const outDir = join(RESULTS_BASE, label);
	const metadataDir = join(outDir, 'metadata');
	mkdirSync(metadataDir, { recursive: true });
	const frameDir = framesDir(label);

	if (!existsSync(VIDEO_PATH)) throw new Error(`Video not found: ${VIDEO_PATH}`);

	console.log(`\n=== OCR Benchmark: ${label} (fps=${fps}, engine=${engine}) ===`);

	console.log('  extracting frames...');
	const { durationS, step, srcFps } = extractFrames(VIDEO_PATH, frameDir, fps);
	const frameFiles = listFrameFiles(frameDir);

	const frameResults = await produceFrameResults({ frameFiles, frameDir, step, srcFps });

	// 累加耗时
	let totalMs = 0, totalDetMs = 0, totalPostMs = 0, totalRecMs = 0;
	for (const fr of frameResults) {
		totalMs += fr.totalMs || 0;
		totalDetMs += fr.detMs || 0;
		totalPostMs += fr.postMs || 0;
		totalRecMs += fr.recMs || 0;
	}
	// Python 模式是 inference_ms 而不是 det/post/rec——取 textScore 为 0，用它来累加 totalMs / 1000
	if (totalMs === 0) {
		// 没有细粒度耗时，使用 inference_s（Python 模式）
		// → 但我们没记录 inference_s 而是 inference_ms ，这里没有
		//
		// => 改为用 textScore 对应的字段 inference_s 由调用者直接返回 totalMs 。
		// 下面 inference_s 为 totalMs / 1000；这和原先一致。
	}

	const segments = mergeFrames(frameResults);
	const mergedText = segments.map(s => s.text).join('');
	const inferenceS = totalMs / 1000;
	const rtf = (durationS > 0) ? inferenceS / durationS : 0;

	const hasPerFrameTimings = totalDetMs > 0 || totalPostMs > 0 || totalRecMs > 0;

	const ocrOutput: any = {
		audio_info: { duration: segments.length > 0 ? segments[segments.length - 1].end : Math.round(durationS * 1000) },
		result: {
			text: mergedText,
			segments: segments.map(s => ({
				text: s.text, start: s.start, end: s.end, confidence: s.confidence,
				...(s.box_y ? { box_y: s.box_y } : {}),
			})),
		},
		_engine: engine,
		_source: 'video_hardsub',
		_fps: fps,
		_textScore: textScore,
		_subtitleOnly: subtitleOnly,
		...extraEngineField,
	};

	if (hasPerFrameTimings) {
		ocrOutput._timingsMs = {
			total: Math.round(totalMs),
			averagePerFrame: Math.round(totalMs / frameResults.length),
			det: Math.round(totalDetMs),
			post: Math.round(totalPostMs),
			rec: Math.round(totalRecMs),
		};
	}

	const ocrPath = join(metadataDir, 'ocr.json');
	writeFileSync(ocrPath, JSON.stringify(ocrOutput, null, 2));

	const cerData = runEvalOCR(ocrPath, label);

	const summary: BenchmarkSummary = {
		label,
		fps,
		engine,
		frames: frameResults.length,
		segments: segments.length,
		audio_duration_s: parseFloat((durationS).toFixed(1)),
		ocr_inference_s: parseFloat(inferenceS.toFixed(3)),
		ocr_rtf: parseFloat(rtf.toFixed(4)),
		wer: cerData.wer ?? 0,
		cer: cerData.cer ?? 0,
		hyp_chars: cerData.hyp_chars ?? mergedText.length,
		ref_chars: cerData.ref_chars ?? 0,
		textScore,
		subtitleOnly,
	};

	if (hasPerFrameTimings) {
		(summary as any).ocr_timings_ms = {
			total: Math.round(totalMs),
			avgPerFrame: Math.round(totalMs / frameResults.length),
			det: Math.round(totalDetMs),
			post: Math.round(totalPostMs),
			rec: Math.round(totalRecMs),
		};
	}

	writeFileSync(join(metadataDir, 'summary.json'), JSON.stringify(summary, null, 2));

	// log output ——和原每个 benchmark 函数相同的 report
	console.log(`  merged ${frameResults.length} frames → ${segments.length} segments`);
	console.log(`  segs=${segments.length} dur=${durationS.toFixed(1)}s`);
	if (hasPerFrameTimings) {
		console.log(`  det=${Math.round(totalDetMs)}ms post=${Math.round(totalPostMs)}ms rec=${Math.round(totalRecMs)}ms total=${Math.round(totalMs)}ms`);
		console.log(`  avg/frame=${Math.round(totalMs / frameResults.length)}ms  inference=${inferenceS.toFixed(3)}s RTF=${rtf.toFixed(4)}`);
	} else {
		console.log(`  OCR inf=${inferenceS.toFixed(1)}s RTF=${rtf.toFixed(4)}`);
	}
	console.log(`  WER=${(cerData.wer * 100).toFixed(2)}% CER=${(cerData.cer * 100).toFixed(2)}%`);
	console.log(`  hyp_chars=${cerData.hyp_chars} ref_chars=${cerData.ref_chars}`);

	spawnSync('rm', ['-rf', frameDir]);

	return summary;
}

function ocrFramePython(framePath: string, textScore?: number, fullFrame?: boolean, subtitleOnly?: boolean): OCRResult {
	const args = [OCR_PY, framePath];
	if (fullFrame) args.push('--full-frame');
	if (subtitleOnly) args.push('--subtitle-only');
	if (textScore != null) args.push('--text-score', String(textScore));
	const r = spawnSync(PYTHON_BIN, args, {
		timeout: 60_000,
		encoding: 'utf-8',
	});
	if (r.status !== 0) {
		console.error(`  OCR warn: exit ${r.status} for ${framePath}`);
		return { lines: [], inferenceMs: 0 };
	}
	const parsed = JSON.parse(r.stdout);
	if (parsed.error) {
		console.error(`  OCR error: ${parsed.error}`);
		return { lines: [], inferenceMs: 0 };
	}
	if (parsed.lines !== undefined) {
		return {
			lines: parsed.lines.map((l: any) => ({ text: l.text, confidence: l.confidence })),
			inferenceMs: parsed.inference_ms ?? 0,
		};
	}
	return { lines: parsed.map((l: any) => ({ text: l.text, confidence: l.confidence })), inferenceMs: 0 };
}

interface CppFrameTiming {
	text: string;
	confidence: number;
	totalMs: number;
	detMs: number;
	postMs: number;
	recMs: number;
}

function ocrFrameCpp(framePath: string, textScore?: number, subtitleOnly?: boolean, customBin?: string, customLdPath?: string, noNms?: boolean): CppFrameTiming {
	const args: string[] = [framePath];
	if (textScore != null) args.push(String(textScore));
	if (subtitleOnly) args.push('--subtitle-only');
	if (noNms) args.push('--no-nms');
	const bin = customBin || CPP_BIN;
	const ldPath = customLdPath || CPP_LD_PATH;
	const r = spawnSync(bin, args, {
		timeout: 60_000,
		encoding: 'utf-8',
		env: { ...process.env, LD_LIBRARY_PATH: ldPath, OCR_MODELS_DIR: OCR_MODELS_DIR, OCR_KEYS_PATH: OCR_KEYS_PATH },
	});
	if (r.status !== 0) {
		console.error(`  C++ OCR error: ${r.stderr?.slice(-200) || `exit ${r.status}`}`);
		return { text: '', confidence: 0, totalMs: 0, detMs: 0, postMs: 0, recMs: 0 };
	}
	const parsed = JSON.parse(r.stdout);
	const segs: { text: string; confidence: number }[] = parsed.segments || [];
	const best = segs.reduce(
		(a, b) => (a.confidence > b.confidence ? a : b),
		{ text: '', confidence: 0 }
	);
	return {
		text: best.text || '',
		confidence: best.confidence || 0,
		totalMs: parsed.totalMs ?? 0,
		detMs: parsed.detInferenceMs ?? 0,
		postMs: parsed.postprocessMs ?? 0,
		recMs: parsed.recInferenceMs ?? 0,
	};
}

function extractFrames(videoPath: string, outDir: string, fps: number = 1): { durationS: number; step: number; srcFps: number } {
	mkdirSync(outDir, { recursive: true });
	const probe = spawnSync(FFPROBE_BIN, [
		'-v', 'error', '-show_entries', 'format=duration',
		'-of', 'csv=p=0', videoPath,
	], { timeout: 15_000, encoding: 'utf-8' });
	const duration = parseFloat(probe.stdout?.trim() || '0');
	if (!duration) throw new Error('Could not probe video duration');

	const frProbe = spawnSync(FFPROBE_BIN, [
		'-v', 'error', '-select_streams', 'v:0', '-show_entries', 'stream=r_frame_rate',
		'-of', 'csv=p=0', videoPath,
	], { timeout: 10_000, encoding: 'utf-8' });
	const frParts = (frProbe.stdout?.trim() || '30/1').split('/');
	const srcFps = parseInt(frParts[0]) / parseInt(frParts[1]);
	const step = Math.round(srcFps / fps);

	const r = spawnSync(FFMPEG_BIN, [
		'-y', '-i', videoPath,
		'-vf', `select='not(mod(n,${step}))'`,
		'-vsync', 'vfr',
		'-qscale:v', '2',
		join(outDir, 'frame_%05d.jpg'),
	], { timeout: 300_000 });
	if (r.status !== 0) {
		throw new Error(`ffmpeg frame extraction failed (exit ${r.status}): ${r.stderr?.toString().slice(-300) || ''}`);
	}

	return { durationS: duration, step, srcFps };
}

function runOCRBenchmarkPython(label: string, fps: number, textScore?: number, subtitleOnly?: boolean) {
	return runBenchmarkCommon(label, fps, 'rapidocr-onnxruntime', { textScore, subtitleOnly },
		({ frameFiles, step, srcFps }) => frameFiles.map((f, i) => {
			const ocr = ocrFramePython(join(framesDir(label), f), textScore, undefined, subtitleOnly);
			const best = selectBest(ocr.lines.map(l => ({ text: l.text, confidence: l.confidence })));
			return {
				text: best.text,
				timestamp: Math.round((i * step / srcFps) * 1000),
				confidence: best.confidence,
				totalMs: ocr.inferenceMs,
			};
		})
	);
}

async function runOCRBenchmarkNode(label: string, fps: number, textScore?: number, subtitleOnly?: boolean) {
	const sessions = await createSessions('cpu');
	// Pre-resolve all async frame results before invoking the sync common template
	const frameDir = framesDir(label);
	const frameFiles = spawnSync('ls', [frameDir], { encoding: 'utf-8' }).stdout.trim().split('\n').filter(f => f.endsWith('.jpg')).sort();
	const resolved: { text: string; confidence: number; totalMs: number }[] = [];
	for (const f of frameFiles) {
		const r = await ocrFrameWithSessions(join(frameDir, f), sessions, { textScore, subtitleOnly });
		const best = r.segments.length > 0 ? selectBest(r.segments) : { text: '', confidence: 0 };
		resolved.push({ text: best.text, confidence: best.confidence, totalMs: r.totalMs });
	}
	const result = runBenchmarkCommon(label, fps, 'ocr-node', { textScore, subtitleOnly },
		({ step, srcFps }) => resolved.map((item, i) => ({
			text: item.text,
			timestamp: Math.round((i * step / srcFps) * 1000),
			confidence: item.confidence,
			totalMs: item.totalMs,
		}))
	);
	await releaseSessions(sessions);
	return result;
}

function runOCRBenchmarkCpp(label: string, fps: number, textScore?: number, subtitleOnly?: boolean) {
	return runBenchmarkCommon(label, fps, 'ort-cpp', { textScore, subtitleOnly },
		({ frameFiles, step, srcFps }) => frameFiles.map((f, i) => {
			const r = ocrFrameCpp(join(framesDir(label), f), textScore, subtitleOnly);
			return {
				text: r.text,
				timestamp: Math.round((i * step / srcFps) * 1000),
				confidence: r.confidence,
				totalMs: r.totalMs,
				detMs: r.detMs,
				postMs: r.postMs,
				recMs: r.recMs,
			};
		})
	);
}

function runOCRBenchmarkCppOpencv(label: string, fps: number, textScore?: number, subtitleOnly?: boolean, noNms?: boolean) {
	return runBenchmarkCommon(label, fps, 'ort-cpp', { textScore, subtitleOnly },
		({ frameFiles, step, srcFps, frameDir }) => {
			const args: string[] = ['--dir', frameDir];
			if (textScore != null) args.push(String(textScore));
			if (subtitleOnly) args.push('--subtitle-only');
			if (noNms) args.push('--no-nms');
			const r = spawnSync(CPP_OPENCV_BIN, args, {
				timeout: 600_000,
				encoding: 'utf-8',
				env: { ...process.env, LD_LIBRARY_PATH: CPP_OPENCV_LD_PATH, OCR_MODELS_DIR, OCR_KEYS_PATH },
			});
			if (r.status !== 0) {
				throw new Error(`cpp-opencv OCR error: ${r.stderr?.slice(-300) || `exit ${r.status}`}`);
			}
			const batchResults: any[] = JSON.parse(r.stdout);
			return frameFiles.map((_, i) => {
				const data = batchResults[i] || { segments: [], text: '' };
				const segs = data.segments || [];
				const best = segs.length > 0 ? selectBest(segs) : { text: '', confidence: 0 };
				return {
					text: best.text || '',
					timestamp: Math.round((i * step / srcFps) * 1000),
					confidence: best.confidence || 0,
					totalMs: (data.detInferenceMs || 0) + (data.postprocessMs || 0) + (data.recInferenceMs || 0),
					detMs: data.detInferenceMs,
					postMs: data.postprocessMs,
					recMs: data.recInferenceMs,
				};
			});
		}
	);
}

function runOCRBenchmarkRust(label: string, fps: number, textScore?: number, subtitleOnly?: boolean) {
	if (!existsSync(RUST_BIN)) throw new Error(`Rust binary not found: ${RUST_BIN}. Run 'cd packages/subtitle-rust && cargo build --release'.`);
	return runBenchmarkCommon(label, fps, 'subtitle-rust', { textScore, subtitleOnly },
		({ frameFiles, step, srcFps, frameDir }) => {
			const args: string[] = ['--dir', frameDir];
			if (textScore != null) args.push(String(textScore));
			if (subtitleOnly) args.push('--subtitle-only');
			const r = spawnSync(RUST_BIN, args, {
				timeout: 600_000,
				encoding: 'utf-8',
				env: { ...process.env, OCR_MODELS_DIR, OCR_KEYS_PATH, OCR_INFER_PY: RUST_INFER_PY },
			});
			if (r.status !== 0) {
				throw new Error(`Rust OCR error: ${r.stderr?.slice(-300) || `exit ${r.status}`}`);
			}
			const batchResults: any[] = JSON.parse(r.stdout);
			return frameFiles.map((_, i) => {
				const data = batchResults[i] || { segments: [], text: '' };
				const segs = data.segments || [];
				const best = segs.length > 0 ? selectBest(segs) : { text: '', confidence: 0 };
				return {
					text: best.text || '',
					timestamp: Math.round((i * step / srcFps) * 1000),
					confidence: best.confidence || 0,
					totalMs: (data.det_inference_ms || 0) + (data.postprocess_ms || 0) + (data.rec_inference_ms || 0),
					detMs: data.det_inference_ms,
					postMs: data.postprocess_ms,
					recMs: data.rec_inference_ms,
				};
			});
		}
	);
}

// ---
if (require.main === module) {
	const engine = process.argv.includes('--engine') ? process.argv[process.argv.indexOf('--engine') + 1] : 'python';
	const onlyLabel = process.argv.includes('--only') ? process.argv[process.argv.indexOf('--only') + 1] : null;
	const runs = process.argv.includes('--runs') ? parseInt(process.argv[process.argv.indexOf('--runs') + 1], 10) : 1;
	globalLabelSuffix = process.argv.includes('--label-suffix') ? process.argv[process.argv.indexOf('--label-suffix') + 1] : null;
	const labelOverride = process.argv.includes('--label-override') ? process.argv[process.argv.indexOf('--label-override') + 1] : null;
	const noNms = process.argv.includes('--no-nms');
	console.log(`Engine: ${engine}  Only: ${onlyLabel ?? 'all'}  Runs: ${runs}${globalLabelSuffix ? `  LabelSuffix: ${globalLabelSuffix}` : ''}${labelOverride ? `  LabelOverride: ${labelOverride}` : ''}${noNms ? `  NMS=OFF` : ''}`);

	async function main() {
		const fpsOptions: { fps: number; textScore?: number; subtitleOnly?: boolean }[] = [
			{ fps: 2, textScore: 0.45, subtitleOnly: true },
			{ fps: 1, textScore: 0.3, subtitleOnly: true },
			{ fps: 1, textScore: 0.4, subtitleOnly: true },
			{ fps: 1, textScore: 0.45, subtitleOnly: true },
			{ fps: 1, textScore: 0.5, subtitleOnly: true },
			{ fps: 1, textScore: 0.45 },
			{ fps: 0.5, textScore: 0.45, subtitleOnly: true },
		];
		const results: any[] = [];

		// Map engine → benchmark function to eliminate 3x duplicated if-else
		const runnerFor: Record<string, (label: string, fps: number, textScore?: number, subtitleOnly?: boolean) => any> = {
			'node': runOCRBenchmarkNode,
			'cpp-opencv': (label, fps, ts, so) => runOCRBenchmarkCppOpencv(label, fps, ts, so, noNms),
			'cpp': runOCRBenchmarkCpp,
			'rust': runOCRBenchmarkRust,
			'python': runOCRBenchmarkPython,
		};
		const runner = runnerFor[engine] || runOCRBenchmarkPython;

		for (const opt of fpsOptions) {
			const tsLabel = opt.textScore != null ? `-ts${opt.textScore}` : '';
			const baseLabel = `ocr-${engine}-fps${opt.fps}${opt.subtitleOnly ? '-so' : ''}${tsLabel}`;
			if (onlyLabel && baseLabel !== onlyLabel) continue;

			if (labelOverride) {
				results.push(await runner(labelOverride, opt.fps, opt.textScore, opt.subtitleOnly));
				break;
			} else if (globalLabelSuffix) {
				results.push(await runner(`${baseLabel}-${globalLabelSuffix}`, opt.fps, opt.textScore, opt.subtitleOnly));
			} else {
				for (let run = 0; run < runs; run++) {
					const label = runs > 1 ? `${baseLabel}-r${run}` : baseLabel;
					results.push(await runner(label, opt.fps, opt.textScore, opt.subtitleOnly));
				}
			}
		}

		console.log('\n======= OCR BENCHMARK SUMMARY =======');
		console.log('label           | fps | ts   | sz  | eng         | frames | segs | dur(s)  | inf(s)  | RTF    | WER%   | CER%   | hyp_ch | ref_ch');
		console.log('----------------|-----|------|-----|-------------|--------|------|---------|---------|--------|--------|--------|-------|-------');
		for (const r of results) {
			const l = r.label.padEnd(16);
			const f = String(r.fps).padStart(3);
			const ts = String(r.textScore ?? 0.5).padStart(4);
			const so = r.subtitleOnly ? 'Y' : 'N';
			const eng = (r.engine || 'python').padEnd(11);
			const fr = String(r.frames).padStart(6);
			const s = String(r.segments).padStart(4);
			const d = String(r.audio_duration_s).padStart(7);
			const inf = r.ocr_inference_s != null ? String(r.ocr_inference_s).padStart(7) : '      —';
			const rtf = r.ocr_rtf != null ? String(r.ocr_rtf).padStart(6) : '     —';
			const w = (r.wer * 100).toFixed(2).padStart(6);
			const c = (r.cer * 100).toFixed(2).padStart(6);
			const hc = String(r.hyp_chars).padStart(7);
			const rc = String(r.ref_chars).padStart(7);
			console.log(`${l} | ${f} | ${ts} |  ${so} | ${eng} | ${fr} | ${s} | ${d} | ${inf} | ${rtf} | ${w}% | ${c}% | ${hc} | ${rc}`);
		}
	}

	main().catch(console.error);
}
