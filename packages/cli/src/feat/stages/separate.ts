import { spawn, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import type { MLDaemon } from '../../ml/daemon/client.ts';
import { Demucs } from './../../ml/demucs/demucs.ts';
import type { Stem } from '../../ml/demucs/load.ts';
import {
	pythonBin,
	REPO_ROOT,
	readConfig,
} from '../config/config.ts';
import { emitLog, ffmpeg, nowISO, probeDuration } from './utils/utils.ts';
import { Context, readCtx, setStage } from '../context/context.ts';

export async function stageSeparate(
	ctx: Context,
	daemon?: MLDaemon,
) {
	const taskId = ctx.task.id;
	const sessionPath = ctx.task.session_path
	// subtitle 模式且未配置 always 时，跳过分离
	const pipeline = ctx?.pipeline || 'dub';
	const sepCfg = ctx.input?.stages?.separate;
	if (pipeline === 'subtitle' && !sepCfg?.always) {
		emitLog(
			sessionPath,
			'[Separate] Skipped (subtitle pipeline, set separate.always=true to force)',
		);
		await setStage(sessionPath, 'separate', {
			status: 'succeeded',
			completed_at: nowISO(),
			progress: 100,
			last_message: 'Skipped (subtitle pipeline)',
		});
		return;
	}

	await setStage(sessionPath, 'separate', {
		last_message: 'Separating audio...',
		progress: 0,
	});

	const videoPath = join(sessionPath, 'media', 'video_source.mp4');
	if (!existsSync(videoPath)) throw new Error('video_source.mp4 not found');

	const runtime = sepCfg?.runtime ?? 'pytorch';
	const device = sepCfg?.device ?? 'cuda';

	if (runtime === 'pytorch' && daemon?.ready) {
		emitLog(sessionPath, `[Separate] Using Python daemon (device=${device})`);
		const absVideo = resolve(
			REPO_ROOT,
			sessionPath,
			'media',
			'video_source.mp4',
		);
		const result = await daemon.runStage(
			'separate',
			taskId,
			{
				video_path: absVideo,
				session_path: sessionPath,
				device,
			},
			(current, _total) => {
				emitLog(sessionPath, `[Separate] ${current}%`);
				setStage(sessionPath, 'separate', {
					progress: current,
					last_message: `Separating ${current}%...`,
				});
			},
		);
		const sr = result as Record<string, number>;
		if (sr.load_time_s)
			emitLog(sessionPath, `[Separate] Model loaded in ${sr.load_time_s}s`);
		if (sr.process_time_s)
			emitLog(sessionPath, `[Separate] Processed in ${sr.process_time_s}s`);
		if (sr.audio_duration_s)
			emitLog(
				taskId,
				`[Separate] Audio duration ${sr.audio_duration_s.toFixed(1)}s`,
			);
		if (sr.rtf) emitLog(sessionPath, `[Separate] RTF ${sr.rtf}`);
	} else if (runtime === 'pytorch') {
		await separatePytorch(taskId, sessionPath, videoPath, device);
	} else if (runtime === 'ggml') {
		await separateGgml(taskId, sessionPath, videoPath, device);
	} else {
		await separateOrt(taskId, sessionPath, videoPath, device);
	}

	await setStage(sessionPath, 'separate', {
		status: 'succeeded',
		completed_at: nowISO(),
		progress: 100,
		last_message: 'Separated',
	});
}

async function separateOrt(
	taskId: string,
	sessionPath: string,
	videoPath: string,
	device: string,
) {
	const ep = device === 'webgpu' ? 'webgpu' : 'cpu';
	const sepCfg = readConfig().stages?.separate;
	const targetStems: Stem[] = sepCfg && 'stems' in sepCfg ? (sepCfg as { stems?: Stem[] }).stems ?? ['vocals'] : ['vocals'];
	emitLog(
		taskId,
		`[Separate] runtime=ort device=${device} stems=${targetStems.join(',')} → ONNX session(${ep})`,
	);

	const demucs = new Demucs(undefined, { executionProvider: ep, stems: targetStems });
	await demucs.load();

	const audioPath = join(sessionPath, 'media', 'audio_source.wav');
	mkdirSync(dirname(audioPath), { recursive: true });
	ffmpeg([
		'-i',
		videoPath,
		'-acodec',
		'pcm_s16le',
		'-ar',
		'44100',
		'-ac',
		'2',
		audioPath,
	]);

	const t0 = performance.now();
	const stems = await demucs.separate(audioPath);
	const elapsedSec = (performance.now() - t0) / 1000;

	emitLog(sessionPath, `[Separate] Processed in ${elapsedSec.toFixed(1)}s`);
	const audioDurationS = stems.vocals.length / 88200;
	emitLog(sessionPath, `[Separate] RTF ${(elapsedSec / audioDurationS).toFixed(2)}`);

	const mediaDir = join(sessionPath, 'media');
	const stemNames = ['drums', 'bass', 'other', 'vocals'] as const;
	for (let i = 0; i < stemNames.length; i++) {
		demucs.writeWav(
			stems[stemNames[i]],
			stems.sampleRate,
			join(mediaDir, `target_${i}_${stemNames[i]}.wav`),
		);
	}
}

async function separatePytorch(
	taskId: string,
	sessionPath: string,
	videoPath: string,
	device: string,
) {
	const scriptPath = join(
		REPO_ROOT,
		'packages',
		'cli',
		'scripts',
		'separate',
		'run.py',
	);
	const pyBin = pythonBin();
	const pythonArgs = [
		scriptPath,
		videoPath,
		resolve(REPO_ROOT, sessionPath),
		'--device',
		device,
	];

	emitLog(sessionPath, `[Separate] runtime=pytorch device=${device}`);

	return new Promise<void>((resolve, reject) => {
		const proc = spawn(pyBin, pythonArgs);

		let stderr = '';

		proc.stdout.on('data', (chunk: Buffer) => {
			const lines = chunk.toString().split('\n').filter(Boolean);
			for (const line of lines) {
				const m = line.match(/^\[PROGRESS\] (\d+)$/);
				if (m) {
					setStage(sessionPath, 'separate', {
						progress: parseInt(m[1]),
						last_message: `Separating ${m[1]}%`,
					});
				}
			}
		});

		proc.stderr.on('data', (chunk: Buffer) => {
			stderr += chunk.toString();
		});

		proc.on('close', (code) => {
			if (code !== 0) {
				reject(
					new Error(`Demucs Python exit code ${code}: ${stderr.slice(-500)}`),
				);
				return;
			}
			resolve();
		});

		proc.on('error', reject);
	});
}

async function separateGgml(
	taskId: string,
	sessionPath: string,
	videoPath: string,
	device: string,
) {
	const ggmlBin = join(
		REPO_ROOT, 'submodule', 'demucs.cpp', 'build', 'demucs_mt.cpp.main',
	);
	const ggmlModel = join(
		REPO_ROOT, 'packages', 'tmp', 'demucs-ggml', 'ggml-model-htdemucs-4s-f16.bin',
	);
	const outDir = resolve(REPO_ROOT, sessionPath, 'tmp', 'ggml-separate');
	mkdirSync(outDir, { recursive: true });

	emitLog(sessionPath, `[Separate] runtime=ggml device=${device} binary=${ggmlBin}`);

	// Extract audio to WAV
	const audioPath = resolve(REPO_ROOT, sessionPath, 'media', 'audio_source.wav');
	mkdirSync(dirname(audioPath), { recursive: true });
	const ffmpegResult = spawnSync('ffmpeg', [
		'-y', '-i', videoPath, '-acodec', 'pcm_s16le', '-ar', '44100', '-ac', '2', audioPath,
	]);
	if (ffmpegResult.status !== 0) {
		throw new Error(`ffmpeg extract failed: ${ffmpegResult.stderr?.toString().slice(-200)}`);
	}

	const isWin = process.platform === 'win32';
	const ggmlBinPath = isWin && !ggmlBin.endsWith('.exe') ? `${ggmlBin}.exe` : ggmlBin;

	if (!existsSync(ggmlBinPath)) {
		emitLog(sessionPath, `[Separate] Binary not found at ${ggmlBinPath}, attempting auto-build...`);
		const built = await tryBuildGgml(sessionPath);
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
		emitLog(sessionPath, `[Separate] Auto-build succeeded`);
	}

	if (!existsSync(ggmlModel)) {
		await ensureGgmlModel(sessionPath, ggmlModel);
	}

	const t0 = performance.now();
	const result = spawnSync(ggmlBinPath, [ggmlModel, audioPath, outDir, '4'], {
		timeout: 600_000,
		env: { ...process.env, OMP_NUM_THREADS: '2' },
	});
	const elapsedSec = (performance.now() - t0) / 1000;

	if (result.error) {
		throw new Error(`GGML separate failed to spawn: ${result.error.message}`);
	}
	if (result.status !== 0) {
		throw new Error(`GGML separate failed (${result.status}): ${result.stderr?.toString().slice(-300)}`);
	}

	emitLog(sessionPath, `[Separate] Processed in ${elapsedSec.toFixed(1)}s`);

	// Copy output WAVs to media/
	const mediaDir = resolve(REPO_ROOT, sessionPath, 'media');
	mkdirSync(mediaDir, { recursive: true });
	const stemNames = ['drums', 'bass', 'other', 'vocals'] as const;
	for (let i = 0; i < stemNames.length; i++) {
		const src = join(outDir, `target_${i}_${stemNames[i]}.wav`);
		const dst = join(mediaDir, `target_${i}_${stemNames[i]}.wav`);
		if (existsSync(src)) {
			copyFileSync(src, dst);
		} else {
			emitLog(sessionPath, `[Separate] WARN: ${src} not found`);
		}
	}

	// Cleanup tmp
	rmSync(outDir, { recursive: true, force: true });

	const durationS = probeDuration(audioPath);
	if (durationS > 0) {
		emitLog(sessionPath, `[Separate] RTF ${(elapsedSec / durationS).toFixed(3)}`);
	}
}

function findCmakePath(): string | null {
	const candidates = [
		join(process.env.ProgramFiles || 'C:\\Program Files', 'CMake', 'bin', 'cmake.exe'),
		join(process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)', 'CMake', 'bin', 'cmake.exe'),
		join(process.env.LOCALAPPDATA || '', 'Microsoft', 'WinGet', 'Links', 'cmake.exe'),
		join(process.env.USERPROFILE || '', 'AppData', 'Local', 'Microsoft', 'WinGet', 'Links', 'cmake.exe'),
	];
	for (const p of candidates) {
		if (existsSync(p)) return p;
	}
	return null;
}

let _cmakePath: string | null = null;

function cmakeBin(): string {
	if (_cmakePath) return _cmakePath;
	const fromPath = spawnSync('where', ['cmake'], { timeout: 5000, shell: true });
	if (fromPath.status === 0) {
		const lines = fromPath.stdout?.toString().trim().split(/\r?\n/);
		if (lines && lines.length > 0 && lines[0].length > 0) {
			_cmakePath = lines[0].trim();
			return _cmakePath;
		}
	}
	const found = findCmakePath();
	if (found) {
		_cmakePath = found;
		return _cmakePath;
	}
	return 'cmake';
}

async function tryBuildGgml(sessionPath: string): Promise<boolean> {
	const log = (msg: string) => { console.log(msg); emitLog(sessionPath, msg); };

	log('[Separate] Checking build prerequisites...');

	if (spawnSync('git', ['--version'], { timeout: 5000 }).status !== 0) {
		log('[Separate] git not found, cannot init submodule');
		return false;
	}
	const cmakeCheck = spawnSync('cmake', ['--version'], { timeout: 5000 });
	if (cmakeCheck.status !== 0) {
		log('[Separate] cmake not found, attempting to install...');
		const isWin = process.platform === 'win32';
		if (isWin) {
			const install = spawnSync('winget', ['install', '--silent', '--accept-package-agreements', 'Kitware.CMake'], {
				timeout: 120_000,
			});
			if (install.status !== 0) {
				log('[Separate] winget install failed (may need admin rights). Install CMake manually: winget install Kitware.CMake');
				return false;
			}
			log('[Separate] CMake installed via winget, probing install location...');
			const found = findCmakePath();
			if (!found) {
				log('[Separate] Could not locate cmake.exe after install.\n'
					+ '  Try running: winget install Kitware.CMake\n'
					+ '  Then restart your terminal and re-run the pipeline.');
				return false;
			}
			_cmakePath = found;
			log(`[Separate] cmake found at ${found}`);
		} else if (process.platform === 'darwin') {
			const install = spawnSync('brew', ['install', 'cmake'], {
				timeout: 120_000,
			});
			if (install.status !== 0) {
				log('[Separate] brew install cmake failed');
				return false;
			}
			log('[Separate] CMake installed via brew');
		} else {
			log('[Separate] Install CMake:\n  Ubuntu/Debian: sudo apt install cmake\n  Fedora:        sudo dnf install cmake\n  Arch:          sudo pacman -S cmake');
			return false;
		}
	}

	const demucsCppDir = join(REPO_ROOT, 'submodule', 'demucs.cpp');
	const buildDir = join(demucsCppDir, 'build');
	let initResult = spawnSync('git', ['submodule', 'update', '--init', 'submodule/demucs.cpp'], {
		cwd: REPO_ROOT,
		timeout: 120_000,
	});
	if (initResult.status !== 0) {
		log('[Separate] SSH submodule init failed, retrying with HTTPS...');
		rmSync(demucsCppDir, { recursive: true, force: true });
		initResult = spawnSync('git', ['clone', '--recurse-submodules', 'https://github.com/sevagh/demucs.cpp.git', demucsCppDir], {
			timeout: 120_000,
		});
		if (initResult.status !== 0) {
			log('[Separate] HTTPS clone also failed');
			return false;
		}
	}

	mkdirSync(buildDir, { recursive: true });

	const isWin = process.platform === 'win32';
	let cmakeGen: string[] = [];
	if (isWin) {
		const hasMSVC = spawnSync('cl', ['/?'], { timeout: 5000 }).status === 0;
		const hasMinGW = spawnSync('g++', ['--version'], { timeout: 5000 }).status === 0;
		if (hasMSVC) {
			cmakeGen = ['-G', 'Visual Studio 17 2022'];
			log('[Separate] Found MSVC (cl.exe), using Visual Studio generator');
		} else if (hasMinGW) {
			cmakeGen = ['-G', 'MinGW Makefiles'];
			log('[Separate] Found MinGW (g++), using MinGW Makefiles generator');
		} else {
			log('[Separate] No C++ compiler found, attempting to install MinGW-w64 via MSYS2...');
			const msysInstall = spawnSync('winget', ['install', '--silent', '--accept-package-agreements', 'MSYS2.MSYS2'], {
				timeout: 120_000,
			});
			if (msysInstall.status !== 0) {
				log('[Separate] MSYS2 winget install failed.\n'
					+ '  Install Visual Studio Build Tools: https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022\n'
					+ '  Or install MSYS2 manually: winget install MSYS2.MSYS2');
				return false;
			}
			const msysCandidates = [
				'C:\\tools\\msys64',
				'C:\\msys64',
				join(process.env.USERPROFILE || '', 'AppData', 'Local', 'MSYS2'),
				join(process.env.LOCALAPPDATA || '', 'MSYS2'),
			];
			const msysRoot = msysCandidates.find(p => existsSync(join(p, 'usr', 'bin', 'pacman.exe')));
			if (!msysRoot) {
				log('[Separate] MSYS2 installed but could not find pacman.exe.\n'
					+ '  Please restart your terminal and run:\n'
					+ '    pacman -S mingw-w64-x86_64-gcc\n'
					+ '  Then add to PATH the mingw64\\bin directory.');
				return false;
			}
			const pacmanPath = join(msysRoot, 'usr', 'bin', 'pacman.exe');
			log('[Separate] Installing mingw-w64-gcc via pacman (may take a while)...');
			const gccInstall = spawnSync(pacmanPath, ['-S', '--noconfirm', 'mingw-w64-x86_64-toolchain'], {
				timeout: 300_000,
			});
			if (gccInstall.status !== 0) {
				log('[Separate] pacman install mingw-w64-toolchain failed.\n'
					+ '  Open MSYS2 clang64.exe and run:\n'
					+ '    pacman -S mingw-w64-x86_64-toolchain');
				return false;
			}
			const mingwBin = join(msysRoot, 'mingw64', 'bin');
			const gppPath = join(mingwBin, 'g++.exe');
			if (!existsSync(gppPath)) {
				log(`[Separate] g++ not found at ${gppPath}.`);
				return false;
			}
			process.env.PATH = `${mingwBin};${process.env.PATH || ''}`;
			log(`[Separate] Added ${mingwBin} to PATH`);
			cmakeGen = ['-G', 'MinGW Makefiles'];
			log('[Separate] Using MinGW Makefiles generator');
		}
	}

	const cmakePath = cmakeBin();
	log(`[Separate] Running cmake configure (${cmakePath})...`);
	const cmakeConfigure = spawnSync(cmakePath, [...cmakeGen, '..', '-DCMAKE_BUILD_TYPE=Release'], {
		cwd: buildDir,
		timeout: 60_000,
	});
	if (cmakeConfigure.status !== 0) {
		const stderr = cmakeConfigure.stderr?.toString() || '(no stderr)';
		log(`[Separate] cmake configure failed:\n${stderr.slice(0, 500)}`);
		return false;
	}

	log(`[Separate] Building binary (may take several minutes)...`);
	const cmakeBuild = spawnSync(cmakePath, ['--build', '.', '--config', 'Release', '-j', '4'], {
		cwd: buildDir,
		timeout: 600_000,
	});
	if (cmakeBuild.status !== 0) {
		const stderr = cmakeBuild.stderr?.toString() || '(no stderr)';
		log(`[Separate] Build failed:\n${stderr.slice(0, 1000)}`);
		return false;
	}

	return true;
}

async function ensureGgmlModel(sessionPath: string, modelPath: string): Promise<void> {
	const modelUrl = 'https://huggingface.co/datasets/Retrobear/demucs.cpp/resolve/main/ggml-model-htdemucs-4s-f16.bin';

	emitLog(sessionPath, `[Separate] Downloading model weights (84 MB) from HuggingFace...`);

	mkdirSync(dirname(modelPath), { recursive: true });

	const response = await fetch(modelUrl);
	if (!response.ok) {
		throw new Error(
			`Failed to download ggml model: HTTP ${response.status}\n`
			+ `Download manually from ${modelUrl}\n`
			+ `and place at ${modelPath}`,
		);
	}

	const buffer = await response.arrayBuffer();
	writeFileSync(modelPath, Buffer.from(buffer));

	emitLog(sessionPath, '[Separate] Model weights downloaded');
}
