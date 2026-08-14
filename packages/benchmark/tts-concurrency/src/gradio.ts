import { readFileSync } from "node:fs";
import { basename } from "node:path";
import type { ProxyFetch } from "./http.ts";
import { readSse } from "./http.ts";

/** /generate 探测到的签名信息 */
export interface GradioSignature {
  apiName: string;
  fnIndex: number;
  /** 输入组件类型列表（按顺序），如 ['textbox','textbox','audio',...] */
  inputTypes: string[];
  /** 输入组件 label（无 label 时用 type） */
  inputLabels: string[];
  gradioVersion: string;
  /** 新版 API 前缀，如 '' 或 '/gradio_api' */
  apiPrefix: string;
  /** 是否支持新版 /call/{api} 接口 */
  supportsCallApi: boolean;
}

export interface GenerateResult {
  ok: boolean;
  /** 成功时结果音频绝对 URL */
  audioUrl?: string;
  latencySec: number;
  /** true 表示被站点拒绝（busy/队列满/不可用） */
  rejected?: boolean;
  error?: string;
}

const BUSY_PATTERN =
  /busy|queue\s*is\s*full|queue full|refused|try again later|server is (too )?busy|rejected|too many/i;

interface ConfigJson {
  version?: string;
  dependencies?: Array<{
    id?: number;
    api_name?: string;
    inputs?: number[];
    outputs?: number[];
  }>;
  components?: Array<{
    id?: number;
    type?: string;
    props?: { label?: string };
  }>;
}

export class GradioClient {
  readonly base: string;
  readonly proxyFetch: ProxyFetch;
  signature?: GradioSignature;
  private sessionHash: string;

  constructor(base: string, proxyFetch: ProxyFetch) {
    this.base = base.replace(/\/+$/, "");
    this.proxyFetch = proxyFetch;
    this.sessionHash = `s_${Math.random().toString(36).slice(2)}${Date.now().toString(36)}`;
  }

  /** 探测 /config，解析 /generate 的组件签名与 API 风格 */
  async probe(): Promise<GradioSignature> {
    const res = await this.proxyFetch(`${this.base}/config`, {
      headers: { "User-Agent": "curl/8.0" },
    });
    if (!res.ok) {
      throw new Error(`GET /config failed: HTTP ${res.status} @ ${this.base}`);
    }
    const cfg = (await res.json()) as ConfigJson;

    const deps = Array.isArray(cfg.dependencies) ? cfg.dependencies : [];
    const dep =
      deps.find((d) => d.api_name === "generate") ??
      deps.find((d) => d.api_name === "predict") ??
      deps.find((d) => d.id === 1 && Array.isArray(d.inputs));
    if (!dep || !Array.isArray(dep.inputs)) {
      throw new Error(`站点 ${this.base} 的 /config 中没有找到 generate/predict 依赖`);
    }

    const compMap = new Map<number, { type: string; label: string }>();
    for (const c of cfg.components ?? []) {
      compMap.set(c.id ?? -1, {
        type: String(c.type ?? "unknown").toLowerCase(),
        label: String(c.props?.label ?? ""),
      });
    }
    const inputTypes = dep.inputs.map((id) => compMap.get(id)?.type ?? "unknown");
    const inputLabels = dep.inputs.map((id) => compMap.get(id)?.label ?? "");

    // 探测 API 前缀与新版 /call 接口
    let apiPrefix = "";
    let supportsCallApi = false;
    if (
      (cfg.version ?? "").startsWith("4") ||
      (cfg.version ?? "").startsWith("5") ||
      (cfg.version ?? "").startsWith("6")
    ) {
      apiPrefix = "/gradio_api";
    }
    // 实测：call 接口可用性
    try {
      const probeRes = await this.proxyFetch(
        `${this.base}${apiPrefix}/call/${dep.api_name ?? "generate"}`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ data: [] }),
        },
      );
      supportsCallApi = probeRes.status === 200 || probeRes.status === 400;
      if (probeRes.status === 404) {
        // 前缀可能不对，再试根路径
        apiPrefix = "";
        const r2 = await this.proxyFetch(`${this.base}/call/${dep.api_name ?? "generate"}`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ data: [] }),
        });
        supportsCallApi = r2.status === 200 || r2.status === 400;
      }
    } catch {
      supportsCallApi = false;
    }

    this.signature = {
      apiName: dep.api_name ?? "generate",
      fnIndex: dep.id ?? 1,
      inputTypes,
      inputLabels,
      gradioVersion: String(cfg.version ?? ""),
      apiPrefix,
      supportsCallApi,
    };
    return this.signature;
  }

  /** 上传参考音频，返回 Gradio FileData 对象（Gradio 6 要求的格式） */
  async uploadAudio(filePath: string): Promise<Record<string, unknown>> {
    const form = new FormData();
    const buf = readFileSync(filePath);
    form.append("files", new Blob([buf], { type: "audio/wav" }), basename(filePath));
    const url = `${this.base}${this.signature?.apiPrefix ?? ""}/upload`;
    const res = await this.proxyFetch(url, { method: "POST", body: form });
    if (!res.ok) {
      throw new Error(`上传音频失败: HTTP ${res.status} @ ${url}`);
    }
    const arr = (await res.json()) as unknown[];
    if (!Array.isArray(arr) || arr.length === 0) {
      throw new Error(`上传音频返回格式异常: ${JSON.stringify(arr)}`);
    }
    const first = arr[0] as string | { path?: string; name?: string };
    const path = typeof first === "string" ? first : (first.path ?? first.name ?? "");
    if (!path) throw new Error("上传音频返回空路径");
    const prefix = this.signature?.apiPrefix ?? "";
    return {
      path,
      url: `${prefix}/file=${path}`,
      orig_name: basename(filePath),
      size: buf.length,
      mime_type: "audio/wav",
      meta: { _type: "gradio.FileData" },
    };
  }

  /**
   * 调用 /generate 并返回结果。
   * data 为按签名顺序构造的参数数组（音频参数已传上传返回的路径字符串）。
   */
  async generate(data: unknown[]): Promise<GenerateResult> {
    if (!this.signature) throw new Error("Call probe() first");
    const t0 = Date.now();
    const sig = this.signature;

    if (sig.supportsCallApi) {
      return this.generateV2(data, t0);
    }
    return this.generateLegacy(data, t0);
  }

  /** 新版：/gradio_api/call/{api} + event_id + SSE */
  private async generateV2(data: unknown[], t0: number): Promise<GenerateResult> {
    const sig = this.signature!;
    const url = `${this.base}${sig.apiPrefix}/call/${sig.apiName}`;
    let callRes: Response;
    try {
      callRes = await this.proxyFetch(url, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ data }),
      });
    } catch (e) {
      return {
        ok: false,
        latencySec: (Date.now() - t0) / 1000,
        error: `call 请求异常: ${e instanceof Error ? e.message : String(e)}`,
      };
    }

    if (callRes.status === 409 || callRes.status === 429 || callRes.status === 503) {
      return {
        ok: false,
        rejected: true,
        latencySec: (Date.now() - t0) / 1000,
        error: `call rejected: HTTP ${callRes.status}`,
      };
    }
    if (!callRes.ok) {
      const text = await callRes.text().catch(() => "");
      if (BUSY_PATTERN.test(text)) {
        return {
          ok: false,
          rejected: true,
          latencySec: (Date.now() - t0) / 1000,
          error: text.slice(0, 200),
        };
      }
      return {
        ok: false,
        latencySec: (Date.now() - t0) / 1000,
        error: `call HTTP ${callRes.status}: ${text.slice(0, 200)}`,
      };
    }

    const j = (await callRes.json().catch(() => ({}))) as { event_id?: string };
    if (!j.event_id) {
      return { ok: false, latencySec: (Date.now() - t0) / 1000, error: "call 响应无 event_id" };
    }

    // SSE 轮询
    const sseUrl = `${this.base}${sig.apiPrefix}/call/${sig.apiName}/${j.event_id}`;
    try {
      const sseRes = await this.proxyFetch(sseUrl, {
        headers: { Accept: "text/event-stream", "User-Agent": "curl/8.0" },
      });
      if (!sseRes.ok || !sseRes.body) {
        return {
          ok: false,
          latencySec: (Date.now() - t0) / 1000,
          error: `SSE HTTP ${sseRes.status}`,
        };
      }
      const events = await readSse(sseRes);
      const completed =
        events.find((e) => e.msg === "process_completed" || e._event === "process_completed") ??
        events.find((e) => e.msg === "complete" || e._event === "complete") ??
        events.find((e) => e._event === "error");
      if (!completed) {
        return { ok: false, latencySec: (Date.now() - t0) / 1000, error: "SSE 无完成事件" };
      }
      const body = (completed.data ?? {}) as {
        success?: boolean;
        output?: { data?: unknown[]; error?: string };
      };
      if (completed._event === "error" || body.success === false) {
        let err = String(body.output?.error ?? "");
        if (!err) {
          err =
            completed.data === null
              ? "generate failed (event:error, data:null)"
              : JSON.stringify(completed.data).slice(0, 300);
        }
        const rejected = BUSY_PATTERN.test(err);
        return {
          ok: false,
          rejected,
          latencySec: (Date.now() - t0) / 1000,
          error: err.slice(0, 300),
        };
      }
      // Gradio 6: complete 事件的 data 直接是结果数组；旧版: output.data
      const rawData = Array.isArray(completed.data) ? completed.data : body.output?.data;
      const output = Array.isArray(rawData) ? (rawData as unknown[]) : [];
      const audio = output[0] ?? {};
      let audioUrl = "";
      if (typeof audio === "string") {
        audioUrl = audio;
      } else if (audio && typeof audio === "object") {
        const a = audio as { url?: string; path?: string };
        audioUrl = a.url ?? a.path ?? "";
      }
      if (!audioUrl) {
        return {
          ok: false,
          latencySec: (Date.now() - t0) / 1000,
          error: `响应无音频 URL: ${JSON.stringify(audio).slice(0, 200)}`,
        };
      }
      return {
        ok: true,
        audioUrl: audioUrl.startsWith("http") ? audioUrl : `${this.base}${audioUrl}`,
        latencySec: (Date.now() - t0) / 1000,
      };
    } catch (e) {
      return {
        ok: false,
        latencySec: (Date.now() - t0) / 1000,
        error: e instanceof Error ? e.message : String(e),
      };
    }
  }

  /** 旧版：/queue/join + /queue/data SSE */
  private async generateLegacy(data: unknown[], t0: number): Promise<GenerateResult> {
    const sig = this.signature!;
    const joinBody = {
      data,
      fn_index: sig.fnIndex,
      session_hash: this.sessionHash,
      trigger_id: sig.fnIndex,
      event_data: null,
    };
    const joinRes = await this.proxyFetch(`${this.base}${sig.apiPrefix}/queue/join`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(joinBody),
    });
    if (joinRes.status === 409 || joinRes.status === 429 || joinRes.status === 503) {
      return {
        ok: false,
        rejected: true,
        latencySec: (Date.now() - t0) / 1000,
        error: `queue join rejected: HTTP ${joinRes.status}`,
      };
    }
    if (!joinRes.ok) {
      const text = await joinRes.text().catch(() => "");
      if (BUSY_PATTERN.test(text)) {
        return {
          ok: false,
          rejected: true,
          latencySec: (Date.now() - t0) / 1000,
          error: text.slice(0, 200),
        };
      }
      return {
        ok: false,
        latencySec: (Date.now() - t0) / 1000,
        error: `queue join HTTP ${joinRes.status}`,
      };
    }

    const sseUrl = `${this.base}${sig.apiPrefix}/queue/data?session_hash=${encodeURIComponent(this.sessionHash)}`;
    try {
      const sseRes = await this.proxyFetch(sseUrl, {
        headers: { Accept: "text/event-stream", "User-Agent": "curl/8.0" },
      });
      if (!sseRes.ok || !sseRes.body)
        return {
          ok: false,
          latencySec: (Date.now() - t0) / 1000,
          error: `SSE HTTP ${sseRes.status}`,
        };
      const events = await readSse(sseRes);
      const completed = events.find(
        (e) =>
          (e.data as { msg?: string } | null)?.msg === "process_completed" ||
          e._event === "process_completed",
      );
      if (!completed)
        return {
          ok: false,
          latencySec: (Date.now() - t0) / 1000,
          error: "SSE 无 process_completed",
        };
      const body = (completed.data ?? {}) as {
        success?: boolean;
        output?: { data?: unknown[]; error?: string };
      };
      if (body.success === false) {
        const err = String(body.output?.error ?? body.output?.error ?? "failed");
        return {
          ok: false,
          rejected: BUSY_PATTERN.test(err),
          latencySec: (Date.now() - t0) / 1000,
          error: err.slice(0, 300),
        };
      }
      const output = body.output?.data ?? [];
      const audio = (output[0] ?? {}) as { url?: string; path?: string };
      const audioUrl = audio.url ?? audio.path;
      if (!audioUrl)
        return { ok: false, latencySec: (Date.now() - t0) / 1000, error: "响应无音频 URL" };
      return {
        ok: true,
        audioUrl: audioUrl.startsWith("http") ? audioUrl : `${this.base}${audioUrl}`,
        latencySec: (Date.now() - t0) / 1000,
      };
    } catch (e) {
      return {
        ok: false,
        latencySec: (Date.now() - t0) / 1000,
        error: e instanceof Error ? e.message : String(e),
      };
    }
  }

  /** 下载音频字节（走同一代理） */
  async downloadAudio(audioUrl: string, timeoutMs = 60000): Promise<Buffer> {
    const ctrl = new AbortController();
    const timer = setTimeout(() => ctrl.abort(), timeoutMs);
    try {
      const res = await this.proxyFetch(audioUrl, { signal: ctrl.signal });
      if (!res.ok) throw new Error(`下载音频 HTTP ${res.status}`);
      const buf = Buffer.from(await res.arrayBuffer());
      if (buf.length <= 1024) throw new Error(`音频文件过小(${buf.length}B)`);
      return buf;
    } finally {
      clearTimeout(timer);
    }
  }
}
