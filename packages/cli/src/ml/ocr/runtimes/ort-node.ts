import * as ort from 'onnxruntime-node';
import { readFileSync, writeFileSync, rmSync, mkdirSync, existsSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { Transformer, ResizeFit } from '@napi-rs/image';
// @ts-ignore - no types published
import { PNG } from 'pngjs';
import { spawnSync } from 'node:child_process';
import { REPO_ROOT, pythonBin } from '../../../feat/config/config.ts';

export interface OCRLine {
	text: string;
	confidence: number;
	box: number[][];
}

/**
 * Dynamically locate rapidocr model directory.
 * On Windows: .venv/Lib/site-packages/rapidocr_onnxruntime/models
 * On Linux:   .venv/lib/python3.x/site-packages/rapidocr_onnxruntime/models
 */
function findRapidOCRModelsDir(): string {
	const venvBase = resolve(REPO_ROOT, '.venv');
	const sitePackagesBase = resolve(venvBase, process.platform === 'win32' ? 'Lib' : 'lib', 'site-packages');
	if (!existsSync(sitePackagesBase)) {
		throw new Error(`site-packages not found: ${sitePackagesBase}`);
	}
	let spDir = sitePackagesBase;
	if (process.platform !== 'win32') {
		const entries = readdirSync(spDir, { withFileTypes: true });
		const pyDir = entries.find(e => e.isDirectory() && e.name.startsWith('python'));
		if (pyDir) {
			spDir = join(spDir, pyDir.name);
		}
	}
	const modelsDir = join(spDir, 'rapidocr_onnxruntime', 'models');
	if (!existsSync(modelsDir)) {
		throw new Error(`rapidocr models not found: ${modelsDir}`);
	}
	return modelsDir;
}

const MODEL_DIR = findRapidOCRModelsDir();
const POSTPROCESS_PY = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'postprocess_det.py');
const TMP_DIR = join(REPO_ROOT, 'packages', 'tmp');

const DET_PATH = join(MODEL_DIR, 'ch_PP-OCRv3_det_infer.onnx');
const CLS_PATH = join(MODEL_DIR, 'ch_ppocr_mobile_v2.0_cls_infer.onnx');
const REC_PATH = join(MODEL_DIR, 'ch_PP-OCRv3_rec_infer.onnx');

const KEYS_PATH = resolve(REPO_ROOT, 'packages', 'subtitle-ocr', 'ppocr_keys.json');
const RAW_CHAR_LIST: string[] = JSON.parse(readFileSync(KEYS_PATH, 'utf-8'));
const CHAR_LIST: string[] = ['', ...RAW_CHAR_LIST.slice(1), ' '];

export interface NodeSessions {
	det: ort.InferenceSession;
	cls: ort.InferenceSession;
	rec: ort.InferenceSession;
}

export type OCRDevice = 'cpu' | 'cuda' | 'directml' | 'coreml' | 'rocm' | 'mps';

function deviceToEp(device: OCRDevice): string {
	switch (device) {
		case 'directml': return 'dml';
		default: return device;
	}
}

export async function createNodeSessions(device: OCRDevice = 'cpu'): Promise<NodeSessions> {
	const ep = deviceToEp(device);
	const [det, cls, rec] = await Promise.all([
		ort.InferenceSession.create(DET_PATH, { executionProviders: [ep] }),
		ort.InferenceSession.create(CLS_PATH, { executionProviders: [ep] }),
		ort.InferenceSession.create(REC_PATH, { executionProviders: [ep] }),
	]);
	return { det, cls, rec };
}

export async function releaseNodeSessions(s: NodeSessions): Promise<void> {
	await Promise.all([s.det.release(), s.cls.release(), s.rec.release()]);
}

function ctcDecode(logits: Float32Array, shape: number[]): { text: string; confidence: number } {
	const [batch, timesteps, numClasses] = shape;
	const totalConf: number[] = [];
	const rawIndices: number[] = [];
	for (let t = 0; t < timesteps; t++) {
		let maxVal = -Infinity;
		let maxIdx = 0;
		for (let c = 0; c < numClasses; c++) {
			const v = logits[t * numClasses + c];
			if (v > maxVal) { maxVal = v; maxIdx = c; }
		}
		rawIndices.push(maxIdx);
		totalConf.push(maxVal);
	}

	const chars: string[] = [];
	const confs: number[] = [];
	let prev = -1;
	for (let i = 0; i < rawIndices.length; i++) {
		const idx = rawIndices[i];
		if (idx === 0) { prev = -1; continue; }
		if (idx !== prev) {
			const ch = CHAR_LIST[idx] ?? '';
			if (ch !== '') { chars.push(ch); confs.push(totalConf[i]); }
		}
		prev = idx;
	}

	const avgConf = confs.length > 0 ? confs.reduce((a, b) => a + b, 0) / confs.length : 0;
	return { text: chars.join(''), confidence: avgConf };
}

// ---------- Image processing (replaces sharp, Windows-friendly) ----------

async function imgSize(buf: Buffer): Promise<{ width: number; height: number }> {
	const meta = await new Transformer(buf).metadata();
	return { width: meta.width, height: meta.height };
}

async function imgCrop(
	buf: Buffer,
	rect: { left: number; top: number; width: number; height: number },
): Promise<Buffer> {
	return new Transformer(buf).crop(rect.left, rect.top, rect.width, rect.height).png();
}

async function imgResizeToRgb(
	buf: Buffer,
	w: number,
	h: number,
): Promise<Buffer> {
	// Bilinear resize → PNG (lossless) → decode → RGBA → RGB
	const pngBuf = await new Transformer(buf).resize(w, h, undefined, ResizeFit.Fill).png();
	const png = PNG.sync.read(pngBuf, { filterType: -1 });
	const rgb = Buffer.alloc(w * h * 3);
	const data = png.data;
	for (let i = 0, j = 0; i < data.length; i += 4, j += 3) {
		rgb[j] = data[i];
		rgb[j + 1] = data[i + 1];
		rgb[j + 2] = data[i + 2];
	}
	return rgb;
}

async function imgRotate180(buf: Buffer): Promise<Buffer> {
	const pngBuf = await new Transformer(buf).png();
	const png = PNG.sync.read(pngBuf, { filterType: -1 });
	const { width, height, data } = png;
	const flipped = Buffer.alloc(data.length);
	for (let y = 0; y < height; y++) {
		for (let x = 0; x < width; x++) {
			const srcIdx = ((height - 1 - y) * width + (width - 1 - x)) * 4;
			const dstIdx = (y * width + x) * 4;
			flipped[dstIdx] = data[srcIdx];
			flipped[dstIdx + 1] = data[srcIdx + 1];
			flipped[dstIdx + 2] = data[srcIdx + 2];
			flipped[dstIdx + 3] = data[srcIdx + 3];
		}
	}
	const out = new PNG({ width, height });
	out.data = flipped;
	return PNG.sync.write(out, { deflateLevel: 9 });
}

// ---------- Preprocessing ----------

async function preprocessDet(
	imgBuf: Buffer,
	opts?: { limitSideLen?: number; bottomOnly?: boolean },
): Promise<{ tensor: Float32Array; origH: number; origW: number; resizedH: number; resizedW: number }> {
	const limitSideLen = opts?.limitSideLen ?? 736;
	const bottomOnly = opts?.bottomOnly ?? false;

	const { width: origW, height: fullH } = await imgSize(imgBuf);

	let inputBuf = imgBuf;
	let origH = fullH;
	if (bottomOnly) {
		const yOffset = Math.floor(fullH * 0.6);
		origH = fullH - yOffset;
		inputBuf = await imgCrop(imgBuf, { left: 0, top: yOffset, width: origW, height: origH });
	}

	let newW: number, newH: number;
	if (origH <= origW) {
		newH = limitSideLen;
		newW = Math.round(origW * limitSideLen / origH);
	} else {
		newW = limitSideLen;
		newH = Math.round(origH * limitSideLen / origW);
	}
	newW = Math.round(newW / 32) * 32;
	newH = Math.round(newH / 32) * 32;

	const resized = await imgResizeToRgb(inputBuf, newW, newH);

	const mean = [0.485, 0.456, 0.406];
	const std = [0.229, 0.224, 0.225];
	const data = new Float32Array(3 * newH * newW);
	for (let y = 0; y < newH; y++) {
		for (let x = 0; x < newW; x++) {
			for (let c = 0; c < 3; c++) {
				const pixel = resized[(y * newW + x) * 3 + c];
				data[c * newH * newW + y * newW + x] = (pixel / 255 - mean[c]) / std[c];
			}
		}
	}

	return { tensor: data, origH, origW, resizedH: newH, resizedW: newW };
}

async function preprocessCls(imgBuf: Buffer): Promise<Float32Array> {
	const resized = await imgResizeToRgb(imgBuf, 192, 48);
	const data = new Float32Array(3 * 48 * 192);
	for (let y = 0; y < 48; y++) {
		for (let x = 0; x < 192; x++) {
			for (let c = 0; c < 3; c++) {
				const pixel = resized[(y * 192 + x) * 3 + c];
				data[c * 48 * 192 + y * 192 + x] = (pixel / 255 - 0.5) / 0.5;
			}
		}
	}
	return data;
}

async function preprocessRec(imgBuf: Buffer): Promise<{ tensor: Float32Array; width: number }> {
	const H = 48;
	const { width: origW, height: origH } = await imgSize(imgBuf);
	const whRatio = origW / origH;
	const imgW = Math.min(320, Math.max(32, Math.round(H * whRatio)));

	const resized = await imgResizeToRgb(imgBuf, imgW, H);

	const data = new Float32Array(3 * H * imgW);
	for (let y = 0; y < H; y++) {
		for (let x = 0; x < imgW; x++) {
			for (let c = 0; c < 3; c++) {
				const pixel = resized[(y * imgW + x) * 3 + c];
				data[c * H * imgW + y * imgW + x] = (pixel / 255 - 0.5) / 0.5;
			}
		}
	}
	return { tensor: data, width: imgW };
}

export async function ocrFrameNode(
	imagePath: string,
	sessions: NodeSessions,
	opts?: { textScore?: number; subtitleOnly?: boolean },
): Promise<OCRLine[]> {
	const textScore = opts?.textScore ?? 0.5;
	const subtitleOnly = opts?.subtitleOnly ?? false;
	const imgBuf = readFileSync(imagePath);

	const { width: fullW, height: fullH } = await imgSize(imgBuf);

	const { tensor: detInput, origH, origW, resizedH, resizedW } = await preprocessDet(imgBuf, { bottomOnly: true });

	const detFeed: Record<string, ort.Tensor> = {};
	detFeed[sessions.det.inputNames[0]] = new ort.Tensor('float32', detInput, [1, 3, resizedH, resizedW]);
	const detOut = await sessions.det.run(detFeed);
	const heatmap = detOut[sessions.det.outputNames[0]] as ort.Tensor;

	// Post-process via Python
	const postDir = join(TMP_DIR, 'ocr-det-' + Math.random().toString(36).slice(2, 8));
	mkdirSync(postDir, { recursive: true });
	const heatmapPath = join(postDir, 'heatmap.raw');
	const heatmapData = heatmap.data as Float32Array;
	writeFileSync(heatmapPath, Buffer.from(heatmapData.buffer, heatmapData.byteOffset, heatmapData.byteLength));

	const pyBin = pythonBin();
	const pp = spawnSync(pyBin, [
		POSTPROCESS_PY,
		heatmapPath,
		String(resizedH),
		String(resizedW),
		String(origH),
		String(origW),
		String(0.3),
		String(textScore),
		String(1.6),
	], { timeout: 30_000, encoding: 'utf-8' });

	try { rmSync(postDir, { recursive: true, force: true }); } catch { }

	if (pp.status !== 0 || !pp.stdout) {
		throw new Error(`Post-process failed: ${pp.stderr?.slice(-200) || 'no output'}`);
	}
	const ppResult = JSON.parse(pp.stdout);
	if (ppResult.error) throw new Error(ppResult.error);
	const boxes: { box: number[][]; score: number }[] = ppResult.boxes ?? [];

	// yOffset = origH*0.6 用于在 ROI 坐标
	const yOffset = Math.floor(fullH * 0.6);

	const segments: OCRLine[] = [];
	for (const b of boxes) {
		const pts = b.box;
		if (!pts || pts.length < 4) continue;

		// 裁剪最小外接轴对齐矩形区域
		const xs = pts.map(p => p[0]);
		const ys = pts.map(p => p[1] + yOffset);
		const xMin = Math.max(0, Math.floor(Math.min(...xs)));
		const xMax = Math.min(fullW, Math.ceil(Math.max(...xs)));
		const yMin = Math.max(0, Math.floor(Math.min(...ys)));
		const yMax = Math.min(fullH, Math.ceil(Math.max(...ys)));

		if (xMax - xMin < 4 || yMax - yMin < 4) continue;

		const crop = await imgCrop(imgBuf, { left: xMin, top: yMin, width: xMax - xMin, height: yMax - yMin });

		const clsInput = await preprocessCls(crop);
		const clsFeed: Record<string, ort.Tensor> = {};
		clsFeed[sessions.cls.inputNames[0]] = new ort.Tensor('float32', clsInput, [1, 3, 48, 192]);
		const clsOut = await sessions.cls.run(clsFeed);
		const clsData = (clsOut[sessions.cls.outputNames[0]] as ort.Tensor).data as Float32Array;
		const rotate = clsData[0] < clsData[1];

		let recCrop = crop;
		if (rotate) recCrop = await imgRotate180(crop);

		const { tensor: recInput, width: recW } = await preprocessRec(recCrop);
		const recFeed: Record<string, ort.Tensor> = {};
		recFeed[sessions.rec.inputNames[0]] = new ort.Tensor('float32', recInput, [1, 3, 48, recW]);
		const recOut = await sessions.rec.run(recFeed);
		const recTensor = recOut[sessions.rec.outputNames[0]] as ort.Tensor;
		const { text, confidence } = ctcDecode(recTensor.data as Float32Array, recTensor.dims as number[]);

		if (!text) continue;
		if (confidence < textScore) continue;

		if (subtitleOnly) {
			const yCenter = (Math.min(...ys) + Math.max(...ys)) / 2;
			if (yCenter < fullH * 0.55) continue;
		}

		const absBox = pts.map(p => [p[0], p[1] + yOffset]);
		segments.push({ text, confidence, box: absBox });
	}

	// Sort top-to-bottom, left-to-right
	const boxCenterY = (b: OCRLine) => {
		const ys = b.box.map(p => p[1]);
		return ys.reduce((a, b) => a + b, 0) / ys.length;
	};
	const boxCenterX = (b: OCRLine) => {
		const xs = b.box.map(p => p[0]);
		return xs.reduce((a, b) => a + b, 0) / xs.length;
	};
	segments.sort((a, b) => {
		const ya = boxCenterY(a), yb = boxCenterY(b);
		if (Math.abs(ya - yb) > 20) return ya - yb;
		return boxCenterX(a) - boxCenterX(b);
	});

	return segments;
}
