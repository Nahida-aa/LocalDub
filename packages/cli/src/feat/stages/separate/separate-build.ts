import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { REPO_ROOT } from '../../config/config.ts';
import { emitLog } from '../utils/utils.ts';

/**
 * Find cmake.exe in common installation paths on Windows.
 */
export function findCmakePath(): string | null {
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

/**
 * Returns the path to cmake binary, checking PATH first, then common install paths.
 */
export function cmakeBin(): string {
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

/**
 * Attempt to build the GGML binary from demucs.cpp submodule.
 * Returns true if build succeeded, false otherwise.
 */
export async function tryBuildGgml(sessionPath: string): Promise<boolean> {
	const log = (msg: string) => { emitLog(sessionPath, msg); };

	log('[Separate] Checking build prerequisites...');

	if (spawnSync('git', ['--version'], { timeout: 5000 }).status !== 0) {
		log('[Separate] git not found, cannot init submodule');
		return false;
	}
	const cmakePath = cmakeBin();
	const cmakeCheck = spawnSync(cmakePath, ['--version'], { timeout: 5000 });
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
	let msysRoot: string | null = null;
	if (isWin) {
		// First, check if MSYS2 already exists and add its paths to PATH
		const msysCandidates = [
			'C:\\tools\\msys64',
			'C:\\msys64',
			join(process.env.USERPROFILE || '', 'AppData', 'Local', 'MSYS2'),
			join(process.env.LOCALAPPDATA || '', 'MSYS2'),
		];
		msysRoot = msysCandidates.find(p => existsSync(join(p, 'usr', 'bin', 'pacman.exe'))) || null;
		const mingwBin = msysRoot ? join(msysRoot, 'mingw64', 'bin') : null;
		const msysBin = msysRoot ? join(msysRoot, 'usr', 'bin') : null;

		// Helper to check compiler via full path (avoid PATH resolution issues)
		const checkCompilerFullPath = (exePath: string) => {
			if (!existsSync(exePath)) return false;
			return spawnSync(exePath, ['--version'], { timeout: 5000 }).status === 0;
		};

		const hasMSVC = checkCompilerFullPath('C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\MSBuild\\Current\\Bin\\MSBuild.exe')
			|| checkCompilerFullPath('C:\\Program Files (x86)\\Microsoft Visual Studio\\2019\\BuildTools\\MSBuild\\Current\\Bin\\MSBuild.exe');
		const hasMinGW = mingwBin ? checkCompilerFullPath(join(mingwBin, 'g++.exe')) : false;

		if (hasMSVC) {
			cmakeGen = ['-G', 'Visual Studio 17 2022'];
			log('[Separate] Found MSVC (cl.exe), using Visual Studio generator');
		} else if (hasMinGW) {
			if (msysBin) process.env.PATH = `${mingwBin};${msysBin};${process.env.PATH || ''}`;
			cmakeGen = ['-G', 'MinGW Makefiles'];
			log('[Separate] Found MinGW (g++), using MinGW Makefiles generator');
		} else if (msysRoot) {
			// MSYS2 exists but g++ not in PATH, install via pacman
			log('[Separate] No C++ compiler found, installing mingw-w64-gcc via pacman...');
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
		} else {
			// No MSYS2 found, try winget install
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
			msysRoot = msysCandidates.find(p => existsSync(join(p, 'usr', 'bin', 'pacman.exe'))) || null;
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

/**
 * Download the GGML model weights from HuggingFace if not present.
 */
export async function ensureGgmlModel(sessionPath: string, modelPath: string): Promise<void> {
	const modelUrl = 'https://huggingface.co/datasets/Retrobear/demucs.cpp/resolve/main/ggml-model-htdemucs-4s-f16.bin';

	emitLog(sessionPath, '[Separate] Downloading model weights (84 MB) from HuggingFace...');

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
