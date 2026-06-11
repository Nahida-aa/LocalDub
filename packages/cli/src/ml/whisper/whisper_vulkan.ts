import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

export interface WhisperWord {
	start: number;  // seconds
	end: number;
	word: string;
	probability?: number;
}

export interface WhisperVulkanSegment {
	text: string;
	start: number;  // seconds
	end: number;
	words: WhisperWord[];
}

export function transcribeWithWords(
	audioPath: string,
	outputJsonPath: string,
	opts?: { language?: string; threads?: number },
): { segments: WhisperVulkanSegment[]; raw: any; elapsedSec: number } {
	const whisperCli = join(
		import.meta.dirname, '..', '..', '..', '..', '..', 'submodule', 'whisper.cpp', 'build', 'bin', 'whisper-vulkan',
	);
	const model = process.env.WHISPER_MODEL || join(
		process.env.HOME || '/root', '.cache', 'pywhispercpp', 'ggml-large-v3-turbo.bin',
	);

	const t0 = performance.now();
	const r = spawnSync(whisperCli, [
		'-m', model,
		audioPath,
		'-l', opts?.language || 'zh',
		'-t', String(opts?.threads ?? 4),
		'-ojf',
	], { timeout: 600_000 });
	const elapsedSec = (performance.now() - t0) / 1000;

	if (r.error) throw new Error(`whisper-vulkan not found: ${whisperCli}\n${r.error.message}`);
	if (r.status !== 0 && r.status !== null) {
		throw new Error(`whisper-vulkan exit ${r.status}: ${r.stderr?.toString().slice(-300)}`);
	}

	const jsonPath = `${audioPath}.json`;
	if (!existsSync(jsonPath)) throw new Error(`whisper did not produce ${jsonPath}`);

	const raw = JSON.parse(readFileSync(jsonPath, 'utf-8'));
	writeFileSync(outputJsonPath, JSON.stringify(raw, null, 2));

	const rawSegments: any[] = Array.isArray(raw.transcription)
		? raw.transcription
		: raw.transcription?.segments ?? [];

	const segments: WhisperVulkanSegment[] = rawSegments.map((s: any) => ({
		text: (s.text || '').trim(),
		start: s.offsets?.from ?? 0,
		end: s.offsets?.to ?? 0,
		words: (s.tokens || [])
			.filter((t: any) => {
				const txt = t.text?.trim();
				return txt && !txt.startsWith('[') && t.offsets?.from != null;
			})
			.map((t: any) => ({
				word: t.text.trim(),
				start: t.offsets.from ?? 0,
				end: t.offsets.to ?? 0,
				probability: t.p,
			})),
	}));

	return { segments, raw, elapsedSec };
}
