import { mkdirSync, writeFileSync, existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type { GradioClient, GenerateResult, GradioSignature } from "./gradio.ts";

export interface Segment {
  segIdx: number;
  text: string;
  wavPath: string;
}

export interface SegmentResult {
  segIdx: number;
  ok: boolean;
  rejected?: boolean;
  latencySec: number;
  error?: string;
  audioPath?: string;
}

export interface TaskResult {
  taskId: string;
  site: string;
  node: string;
  port: number;
  segments: SegmentResult[];
  startAt: string;
  endAt?: string;
  totalSec?: number;
  okCount: number;
  failCount: number;
  firstRejectedAt?: number;
}

export interface TaskOptions {
  /** 每任务最多跑前 N 段（用于快速验证，0=全部） */
  maxSegments: number;
  /** 单请求超时秒 */
  requestTimeoutSec: number;
  /** 失败重试次数 */
  retries: number;
  /** 音频输出目录 */
  audioDir: string;
  /** 结果落盘目录 */
  resultDir: string;
  /** 每段完成后回调 */
  onSegment?: (r: SegmentResult) => void;
  /** 站点级停止条件（busy 时其他任务每段前检查） */
  shouldStop?: () => boolean;
}

/** 按签名动态构造 /generate 参数数组 */
export function buildArgs(
  sig: GradioSignature,
  text: string,
  refFile: Record<string, unknown> | null,
): unknown[] {
  const args: unknown[] = [];
  const seen = { text: 0, audio: 0 };
  for (let i = 0; i < sig.inputTypes.length; i++) {
    const type = sig.inputTypes[i];
    const label = (sig.inputLabels[i] ?? "").toLowerCase();
    if (type.includes("text")) {
      // 第一个文本框放文本，其余放 control 指令/提示词
      seen.text++;
      args.push(seen.text === 1 ? text : "");
    } else if (type.includes("audio")) {
      seen.audio++;
      args.push(seen.audio === 1 ? refFile : null);
    } else if (type.includes("checkbox")) {
      args.push(false);
    } else if (type.includes("number") || type.includes("slider")) {
      if (label.includes("cfg") || label.includes("temperature")) args.push(2.0);
      else if (label.includes("step") || label.includes("dit") || label.includes("seed"))
        args.push(10);
      else if (label.includes("speed")) args.push(1.0);
      else args.push(1);
    } else if (type.includes("dropdown") || type.includes("radio")) {
      args.push("none");
    } else {
      args.push("");
    }
  }
  return args;
}

/** 从 split_audio.json 构造任务段列表 */
export function loadSegments(
  splitAudioJson: string,
  vocalsDir: string,
  maxSegments: number,
): Segment[] {
  const doc = JSON.parse(readFileSync(splitAudioJson, "utf8")) as {
    translation: Array<{ seg_idx: number; dst: string }>;
  };
  const segs: Segment[] = [];
  for (const t of doc.translation) {
    const idx = t.seg_idx;
    const wavPath = join(vocalsDir, `${String(idx).padStart(4, "0")}.wav`);
    segs.push({ segIdx: idx, text: t.dst, wavPath });
    if (maxSegments > 0 && segs.length >= maxSegments) break;
  }
  return segs;
}

/** 串行跑完一个任务的所有段（支持断点续跑：已完成段跳过） */
export async function runTask(
  client: GradioClient,
  taskId: string,
  site: string,
  nodeName: string,
  port: number,
  segments: Segment[],
  opts: TaskOptions,
): Promise<TaskResult> {
  const resultFile = join(opts.resultDir, `${site}-${taskId}.json`);
  let task: TaskResult = {
    taskId,
    site,
    node: nodeName,
    port,
    segments: [],
    startAt: new Date().toISOString(),
    okCount: 0,
    failCount: 0,
  };
  if (existsSync(resultFile)) {
    try {
      const prev = JSON.parse(readFileSync(resultFile, "utf8")) as TaskResult;
      task = { ...prev, segments: [], okCount: 0, failCount: 0 };
    } catch {
      // 损坏的续跑文件，忽略
    }
  }

  const t0 = Date.now();
  for (const seg of segments) {
    if (opts.shouldStop?.()) {
      console.log(`[${site}] 站点已停止，任务 ${taskId} 在段 ${seg.segIdx} 前终止。`);
      break;
    }
    const segResult = await runSegment(client, seg, opts, audioDirFor(taskId, opts));
    task.segments.push(segResult);
    if (segResult.ok) task.okCount++;
    else task.failCount++;
    if (segResult.rejected && task.firstRejectedAt === undefined) {
      task.firstRejectedAt = seg.segIdx;
    }
    opts.onSegment?.(segResult);
    // 增量落盘
    task.totalSec = (Date.now() - t0) / 1000;
    writeFileSync(resultFile, JSON.stringify(task, null, 2));
    // 站点拒绝则终止任务
    if (segResult.rejected) break;
  }
  task.endAt = new Date().toISOString();
  task.totalSec = (Date.now() - t0) / 1000;
  writeFileSync(resultFile, JSON.stringify(task, null, 2));
  return task;
}

async function runSegment(
  client: GradioClient,
  seg: Segment,
  opts: TaskOptions,
  taskAudioDir: string,
): Promise<SegmentResult> {
  const base: SegmentResult = { segIdx: seg.segIdx, ok: false, latencySec: 0 };
  let refFile: Record<string, unknown> | null = null;
  try {
    refFile = await client.uploadAudio(seg.wavPath);
  } catch (e) {
    base.error = `上传失败: ${e instanceof Error ? e.message : String(e)}`;
    return base;
  }
  if (!refFile || !refFile.path) {
    base.error = "上传返回空路径";
    return base;
  }

  const sig = client.signature!;
  const args = buildArgs(sig, seg.text, refFile);

  let lastErr: string | undefined;
  let rejected = false;
  for (let attempt = 0; attempt <= opts.retries; attempt++) {
    const result: GenerateResult = await withTimeout(client.generate(args), opts.requestTimeoutSec);
    if (result.ok) {
      try {
        const buf = await client.downloadAudio(result.audioUrl!);
        const outDir = taskAudioDir;
        mkdirSync(outDir, { recursive: true });
        const outPath = join(outDir, `${String(seg.segIdx).padStart(4, "0")}.wav`);
        writeFileSync(outPath, buf);
        return { segIdx: seg.segIdx, ok: true, latencySec: result.latencySec, audioPath: outPath };
      } catch (e) {
        lastErr = `下载失败: ${e instanceof Error ? e.message : String(e)}`;
        // 下载失败可重试
        if (attempt < opts.retries) continue;
      }
    } else {
      lastErr = result.error ?? "generate failed";
      rejected = result.rejected ?? false;
      if (rejected) break; // 站点拒绝不重试
      if (attempt >= opts.retries) break;
      await new Promise((r) => setTimeout(r, 2000)); // 失败重试间隔
    }
  }
  return { segIdx: seg.segIdx, ok: false, rejected, latencySec: 0, error: lastErr };
}

function audioDirFor(taskId: string, opts: TaskOptions): string {
  return join(opts.audioDir, taskId);
}

function withTimeout<T>(p: Promise<T>, seconds: number): Promise<T> {
  return Promise.race([
    p,
    new Promise<never>((_, rej) => {
      setTimeout(() => rej(new Error(`请求超时(>${seconds}s)`)), seconds * 1000);
    }),
  ]);
}
