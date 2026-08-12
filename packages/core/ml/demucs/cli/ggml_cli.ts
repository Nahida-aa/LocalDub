import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { homedir } from 'node:os';
import { DEMUCS_GGML_FILE } from '@repo/config/path/models';
import { emitLog, separateDir } from '@repo/core/stages/utils/utils';
import { probeDurationMs } from '@repo/core/utils/ffmpeg';
import { setStage } from '@repo/core/context/context';
import { DemucsCliArgs } from './cli_types';
import { ensureGgmlModel, tryBuildGgml } from '../separate-build';
import { REPO_ROOT } from '@repo/config/root';

export async function separateGgml(
	taskId: string,
	taskDir: string,
	audioPath: string,
	device: string,
) {
	const ggmlBin = join(
		REPO_ROOT, 'submodule', 'demucs.cpp', 'build', 'demucs_mt.cpp.main',
	);
	const ggmlModel = DEMUCS_GGML_FILE;
	const sepDir = separateDir(taskDir);
	mkdirSync(sepDir, { recursive: true });

	emitLog(taskDir, `[Separate] runtime=ggml device=${device} binary=${ggmlBin}`);

	// Extract audio to WAV
	if (!existsSync(audioPath)) throw new Error('audio_source.wav not found (run download stage)');

	const isWin = process.platform === 'win32';
	const ggmlBinPath = isWin && !ggmlBin.endsWith('.exe') ? `${ggmlBin}.exe` : ggmlBin;

	if (!existsSync(ggmlBinPath)) {
		emitLog(taskDir, `[Separate] Binary not found at ${ggmlBinPath}, attempting auto-build...`);
		const built = await tryBuildGgml(taskDir);
		if (!built) {
			throw new Error(
				`GGML binary not found at ${ggmlBinPath}\n`
				+ `Auto-build failed. To build manually:\n`
				+ `  1. git submodule update --init submodule/demucs.cpp\n`
				+ `  2. cd submodule/demucs.cpp && mkdir build && cd build\n`
				+ `  3. cmake .. && cmake --build . --config Release -j4\n`
				+ `Or set separate.runtime to "ort" or "pytorch" in config to use ONNX or Python instead.`,
			);
		}
		emitLog(taskDir, `[Separate] Auto-build success`);
	}

	if (!existsSync(ggmlModel)) {
		await ensureGgmlModel(taskDir, ggmlModel);
	}

	const t0 = performance.now();
	await new Promise<void>((resolve, reject) => {
		const proc = spawn(ggmlBinPath, [ggmlModel, audioPath, sepDir, '4'], {
			env: { ...process.env, OMP_NUM_THREADS: '2' },
		});
		let stderr = '';
		const hung = setTimeout(() => {
			proc.kill('SIGKILL');
			reject(new Error('GGML separate timed out after 600s'));
		}, 600_000);

		let lastGgmlPct = -1;
		proc.stdout?.on('data', (chunk) => {
			const lines = chunk.toString().split('\n');
			for (const line of lines) {
				const m = line.match(/\((\s*\d+(?:\.\d+)?)%\)/);
				if (m) {
					const pct = Math.min(100, Math.max(0, Math.round(Number(m[1]))));
					if (pct === lastGgmlPct) continue;
					lastGgmlPct = pct;
					setStage(taskDir, 'separate', {
						progress: pct,
						last_message: `Separating ${pct}%`,
					});
				}
			}
		});

		proc.stderr?.on('data', (chunk) => {
			stderr += chunk.toString();
		});

		proc.on('error', (e) => {
			clearTimeout(hung);
			reject(new Error(`GGML separate failed to spawn: ${e.message}`));
		});

		proc.on('close', (code) => {
			clearTimeout(hung);
			if (code === 0) resolve();
			else reject(new Error(`GGML separate failed (${code}): ${stderr.slice(-300)}`));
		});
	});
	const elapsedSec = (performance.now() - t0) / 1000;

	emitLog(taskDir, `[Separate] Processed in ${elapsedSec.toFixed(1)}s`);

	const stemNames = ['drums', 'bass', 'other', 'vocals'] as const;
	for (const name of stemNames) {
		const p = join(sepDir, `target_${stemNames.indexOf(name)}_${name}.wav`);
		if (!existsSync(p)) {
			emitLog(taskDir, `[Separate] WARN: ${p} not found`);
		}
	}

	const durationMs = probeDurationMs(audioPath);
	if (durationMs > 0) {
		emitLog(taskDir, `[Separate] RTF ${(elapsedSec / (durationMs / 1000)).toFixed(3)}`);
	}
}
