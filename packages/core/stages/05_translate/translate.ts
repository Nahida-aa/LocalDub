import { readJson, writeJson, ensureDir } from '@repo/core/utils/fileOps';
import { existsSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { env } from '@repo/config/env';
import { readInputArgs, } from '@repo/core/input/input';
import {
	emitLog,
	LANG_NAMES,
	nowISO,
	readTaskLanguages,
	SrtJson,
	subtitleFilePath,
	translationFilePath,
} from '@repo/core/stages/utils/utils.ts';
import { TaskCtx, setCtx, setStage } from '@repo/core/context/context.ts';
import { buildPreprocessPrompt, buildTranslateSystem, resolveTargetLanguage } from './utils';
import { chat_completions } from '../../ml/llm/openai';
import { to } from '@repo/shared/lib/utils/try';
import { TranslateFile } from './type';


export async function stageTranslate(ctx: TaskCtx) {
	const taskId = ctx.task.id;
	const taskDir = ctx.task.task_dir
	const { asrLanguage: srcLangCode, } = readTaskLanguages(ctx);

	const dstLangCode = resolveTargetLanguage(ctx);
	const translationFile = translationFilePath(taskDir, dstLangCode);
	const srcLangName = LANG_NAMES[srcLangCode] || srcLangCode;
	const dstLangName = LANG_NAMES[dstLangCode] || dstLangCode;

	const srtFile = subtitleFilePath(ctx);
	const data = await readJson<SrtJson>(srtFile, ctx);
	const segments = data.result.segments;
	const texts = segments.map((u: any) => (u.text || '').trim());
	const fullText = (data.result.text || '').trim() || texts.join(' ');

	const ytdlpPath = join(taskDir, 'download', 'ytdlp_info.json');
	const hasMeta = existsSync(ytdlpPath);
	let meta: any = {};
	if (hasMeta) {
		meta = readJson(ytdlpPath, ctx);
	}

	const transArgs = readInputArgs().stages?.translate;
	const apiKey = env.OPENAI_API_KEY;
	if (!apiKey) throw new Error('OPENAI_API_KEY not configured');

	const metaView = {
		title: (meta.title || '').trim().slice(0, 500) || '(unknown)',
		uploader: (meta.uploader || '').trim().slice(0, 200) || '(unknown)',
		description: (meta.description || '').trim().slice(0, 500) || '(none)',
	};

	let preprocessPrompt = '';
	if (hasMeta) {
    preprocessPrompt = buildPreprocessPrompt({
      dstLangName,
      srcLangName,
      metaView,
      fullText,
		})
	}

	async function callJson(
		system: string,
		user: string,
		maxTokens = 1024,
	): Promise<any> {
		const raw = await chat_completions(user, {
      systemPrompt: system,
      max_tokens: maxTokens,
      api_key: apiKey,
      apiBase: transArgs.apiBase,
      model: transArgs.model,
			temperature: 0.2,
    });
    const [ret, err] = to(() => JSON.parse(raw))
    if (ret) return ret
    const m = raw.match(/\{.*\}/s);
    if (m) return JSON.parse(m[0]);
    // fallback: LLM returned numbered list instead of JSON (e.g. deepseek-v4-flash)
    const numbered = raw.match(/^\d+[.)、]\s*(.+)$/gm);
    if (numbered && numbered.length > 0) {
      return { dst: numbered.map((l) => l.replace(/^\d+[.)、]\s*/, '').trim()) };
    }
    throw new Error(
			`Failed to parse JSON from LLM response: ${raw.slice(0, 300)}`,
		);
	}

	let summary = '',
		hotwords: string[] = [],
		corrections: string[] = [];
	if (hasMeta) {
		try {
			const pre = await callJson(
				'You output strict JSON only.',
				preprocessPrompt,
				2048,
			);
			summary = pre.summary || '';
			hotwords = (pre.hotwords || []).map((h: any) => `${h.src} -> ${h.dst}`);
			corrections = (pre.corrections || []).map(
				(c: any) => `${c.wrong} -> ${c.correct}`,
			);
		} catch (e: any) {
			emitLog(taskDir, `[WARN] [Translate] Preprocess failed: ${e.message}`);
		}
	}

	const hotwordsStr = hotwords.length ? hotwords.join('\n') : '(none)';
	const correctionsStr = corrections.length ? corrections.join('\n') : '(none)';

	const translateSystem = buildTranslateSystem({
		dstLangName,
		srcLangName,
		metaView,
		summary,
		hotwordsStr,
		correctionsStr,
	});

	const BATCH_SIZE = 50;
	const dsts: string[] = [];

	async function translateBatch(
		batchTexts: string[],
		attempt = 0,
	): Promise<string[]> {
		const numbered = batchTexts.map((t, i) => `${i + 1}. ${t}`).join('\n');
		const userMsg =
			attempt > 0
				? `${numbered}\n\n（注意：以上回复包含中文！必须全部输出${dstLangName}译文，不得包含任何中文。）`
				: numbered;
		try {
			const data = await callJson(translateSystem, userMsg, 3072);
			console.log(`[translate] Batch translated:`, data);
			const arr = data.dst;
			if (!Array.isArray(arr) || arr.length === 0)
				throw new Error('dst is not an array');
			const results = arr
				.slice(0, batchTexts.length)
				.map((d: any, i: number) => {
					const dst = String(d ?? '').trim();
					const chineseRatio =
						(dst.match(/[\u4e00-\u9fff]/g) || []).length / (dst.length || 1);
					if (dstLangCode !== 'zh' && chineseRatio > 0.3) {
						const msg = `[Translate] Item ${i + 1} still Chinese (ratio=${chineseRatio.toFixed(2)}, expected ${dstLangCode})`;
						emitLog(taskDir, `[ERROR] ${msg}`);
						throw new Error(msg);
					}
					if (!dst) {
						const msg = `[Translate] Item ${i + 1} got empty dst`;
						emitLog(taskDir, `[ERROR] ${msg}`);
						throw new Error(msg);
					}
					return dst;
				});
			if (results.length < batchTexts.length) {
				const msg = `[Translate] batch produced ${results.length} translations for ${batchTexts.length} inputs`;
				emitLog(taskDir, `[ERROR] ${msg}`);
				throw new Error(msg);
			}
			return results;
		} catch (e: any) {
			if (attempt < 2) return translateBatch(batchTexts, attempt + 1);
			const msg = `[Translate] batch failed after 3 attempts: ${e.message || e} (expected ${dstLangCode})`;
			emitLog(taskDir, `[ERROR] ${msg}`);
			throw new Error(msg);
		}
	}

	for (let i = 0; i < texts.length; i += BATCH_SIZE) {
		const batch = texts.slice(i, i + BATCH_SIZE);
		const results = await translateBatch(batch);
		dsts.push(...results);
		await setStage(taskDir, 'translate', {
			last_message: `Translating ${Math.min(i + BATCH_SIZE, texts.length)}/${texts.length}...`,
		});
	}

	const translation: TranslateFile['translation'] = segments.map((u: any, idx: number) => ({
		src: texts[idx],
		dst: dsts[idx]?.replace(/——/g, '，') || '',
		src_lang: srcLangCode,
		dst_lang: dstLangCode,
		start: u.start,
		end: u.end,
		speaker: '1',
	}));

	const translateDir = join(taskDir, 'translate');
	ensureDir(translateDir, ctx);
	writeJson(translationFile, { translation }, ctx);

	await setStage(taskDir, 'translate', {
		status: 'success',
		completed_at: nowISO(),
		progress: 100,
		last_message: 'Translated',
	});
}
