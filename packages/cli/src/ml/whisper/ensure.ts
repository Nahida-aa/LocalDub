import { existsSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { homedir } from 'node:os';
import { REPO_ROOT } from '../../feat/config/config.ts';
import { emitLog } from '../../feat/stages/utils/utils.ts';

const HF_BASE = 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main';

function whisperCppDir(): string {
	return join(REPO_ROOT, 'submodule', 'whisper.cpp');
}

function modelsDir(): string {
	return join(whisperCppDir(), 'models');
}

function vadModelFilename(): string {
	return 'ggml-silero-vad-v6.2.0.bin';
}

function whisperModelPath(): string {
	if (process.platform === 'win32') {
		return join(homedir(), 'AppData', 'Local', 'pywhispercpp', 'ggml-large-v3-turbo.bin');
	}
	return join(homedir(), '.cache', 'pywhispercpp', 'ggml-large-v3-turbo.bin');
}

export function whisperVulkanPath(): string {
	// Prefer Release or bin/Release copies then common locations.
	const baseBin = join(whisperCppDir(), 'build', 'bin');
	const buildRoot = join(whisperCppDir(), 'build');

	const candidates: string[] = [];
	if (process.platform === 'win32') {
		candidates.push(join(buildRoot, 'Release', 'whisper-cli.exe'));
		candidates.push(join(baseBin, 'Release', 'whisper-cli.exe'));
		candidates.push(join(baseBin, 'whisper-cli.exe'));
		candidates.push(join(baseBin, 'whisper-vulkan.exe'));
		candidates.push(join(buildRoot, 'whisper-cli.exe'));
		candidates.push(join(buildRoot, 'bin', 'whisper-cli.exe'));
		candidates.push(join(buildRoot, 'bin', 'whisper-vulkan.exe'));
		candidates.push(join(buildRoot, 'Release', 'whisper-vulkan.exe'));
	} else {
		candidates.push(join(buildRoot, 'Release', 'whisper-cli'));
		candidates.push(join(baseBin, 'Release', 'whisper-cli'));
		candidates.push(join(baseBin, 'whisper-cli'));
		candidates.push(join(baseBin, 'whisper-vulkan'));
		candidates.push(join(buildRoot, 'whisper-cli'));
		candidates.push(join(buildRoot, 'bin', 'whisper-cli'));
		candidates.push(join(buildRoot, 'bin', 'whisper-vulkan'));
		candidates.push(join(buildRoot, 'Release', 'whisper-vulkan'));
	}

	for (const c of candidates) {
		if (existsSync(c)) return c;
	}

	// Fallback to original expected path
	const fallback = process.platform === 'win32'
		? join(whisperCppDir(), 'build', 'bin', 'whisper-vulkan.exe')
		: join(whisperCppDir(), 'build', 'bin', 'whisper-vulkan');
	return fallback;
}

export function whisperCppBinaryPath(): string {
	return whisperVulkanPath();
}

function downloadFile(url: string, dest: string, sessionPath: string): boolean {
	const destDir = join(dest, '..');
	if (!existsSync(destDir)) mkdirSync(destDir, { recursive: true });

	emitLog(sessionPath, `[Whisper] Downloading ${url.split('/').pop()}...`);
	const r = spawnSync('curl.exe', ['-#L', '-o', dest, url], { timeout: 300_000 });
	if (r.status !== 0) {
		emitLog(sessionPath, `[Whisper] Download failed (curl exit ${r.status})`);
		return false;
	}
	if (!existsSync(dest)) {
		emitLog(sessionPath, `[Whisper] File not found after download: ${dest}`);
		return false;
	}
	emitLog(sessionPath, `[Whisper] Saved to ${dest}`);
	return true;
}

export function ensureWhisperCppSubmodule(sessionPath: string): boolean {
	const gitDir = join(whisperCppDir(), '.git');
	if (existsSync(gitDir)) return true;

	emitLog(sessionPath, '[Whisper] Initializing whisper.cpp submodule...');
	const r = spawnSync('git', ['submodule', 'update', '--init', 'submodule/whisper.cpp'], {
		timeout: 120_000,
		cwd: REPO_ROOT,
	});
	if (r.status !== 0) {
		emitLog(sessionPath, '[Whisper] Failed to init submodule. Run manually:\n'
			+ `  git submodule update --init submodule/whisper.cpp`);
		return false;
	}
	emitLog(sessionPath, '[Whisper] Submodule initialized');
	return true;
}

export function ensureWhisperCppModel(sessionPath: string): boolean {
	const dest = whisperModelPath();
	if (existsSync(dest)) return true;

	const url = `${HF_BASE}/ggml-large-v3-turbo.bin`;
	emitLog(sessionPath, `[Whisper] Whisper model not found at ${dest}`);
	return downloadFile(url, dest, sessionPath);
}

export function ensureVadModel(sessionPath: string): boolean {
	const dest = join(modelsDir(), vadModelFilename());
	if (existsSync(dest)) return true;

	if (!ensureWhisperCppSubmodule(sessionPath)) return false;

	const url = `${HF_BASE}/${vadModelFilename()}`;
	emitLog(sessionPath, `[Whisper] VAD model not found at ${dest}`);
	return downloadFile(url, dest, sessionPath);
}

// ---- Vulkan SDK helper & installer (user-provided logic) ----
const VK_SDK_VERSION = '1.3.296.0';
const VK_SDK_INSTALLER_URL =
	`https://sdk.lunarg.com/sdk/download/${VK_SDK_VERSION}/windows/VulkanSDK-${VK_SDK_VERSION}-Installer.exe`;

let _vkSdkDir: string | null = null;

function findVulkanSdkDir(): string | null {
	if (_vkSdkDir && existsSync(join(_vkSdkDir, 'Include', 'vulkan', 'vulkan.h'))) return _vkSdkDir;

	const candidates = [
		process.env.VK_SDK_PATH,
		join('C:\\', 'VulkanSDK', VK_SDK_VERSION),
		join(process.env.ProgramFiles || 'C:\\Program Files', 'VulkanSDK', VK_SDK_VERSION),
		join(process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)', 'VulkanSDK', VK_SDK_VERSION),
	];
	for (const dir of candidates) {
		if (dir && existsSync(join(dir, 'Include', 'vulkan', 'vulkan.h'))) {
			_vkSdkDir = dir;
			return dir;
		}
	}
	return null;
}

function isVulkanSdkInstalled(): boolean {
	return findVulkanSdkDir() !== null;
}

function installVulkanSdkWinget(sessionPath: string): boolean {
	const tmpDir = join(REPO_ROOT, 'packages', 'tmp');
	const installerPath = join(tmpDir, `VulkanSDK-${VK_SDK_VERSION}-Installer.exe`);

	emitLog(sessionPath, '[Whisper] Installing Vulkan SDK via winget...');
	emitLog(sessionPath, '[Whisper] This may open a UAC prompt (admin required)');
	const r = spawnSync('winget', ['install', '-e', '--id', 'KhronosGroup.VulkanSDK', '--accept-package-agreements'],
		{ timeout: 600_000 });
	if (r.status !== 0) {
		const err = r.stderr?.toString() || '';
		emitLog(sessionPath, `[Whisper] winget install failed (exit ${r.status}):\n${err.slice(-300)}`);
		return false;
	}
	emitLog(sessionPath, '[Whisper] Vulkan SDK installed via winget');
	return true;
}

function installVulkanSdkInstaller(sessionPath: string): boolean {
	const tmpDir = join(REPO_ROOT, 'packages', 'tmp');
	const installerPath = join(tmpDir, `VulkanSDK-${VK_SDK_VERSION}-Installer.exe`);

	if (!downloadFile(VK_SDK_INSTALLER_URL, installerPath, sessionPath)) return false;

	emitLog(sessionPath, `[Whisper] Running Vulkan SDK installer (${VK_SDK_VERSION})...`);
	emitLog(sessionPath, '[Whisper] This may open a UAC prompt (admin required)');
	const r = spawnSync(`"${installerPath}"`, ['/S'], {
		timeout: 600_000,
		shell: true,
	});
	if (r.status !== 0) {
		emitLog(sessionPath, `[Whisper] Vulkan SDK installer failed (exit ${r.status})`);
		emitLog(sessionPath, `[Whisper] Install manually: ${VK_SDK_INSTALLER_URL}`);
		return false;
	}
	emitLog(sessionPath, '[Whisper] Vulkan SDK installed');
	return true;
}

function ensureVulkanSdk(sessionPath: string): boolean {
	if (isVulkanSdkInstalled()) {
		emitLog(sessionPath, '[Whisper] Vulkan SDK OK');
		return true;
	}

	emitLog(sessionPath, '[Whisper] Vulkan SDK not found');
	if (installVulkanSdkWinget(sessionPath) && isVulkanSdkInstalled()) return true;
	if (installVulkanSdkInstaller(sessionPath) && isVulkanSdkInstalled()) return true;

	emitLog(sessionPath, '[Whisper] Vulkan SDK install failed. Install manually:\n'
	+ `  winget install -e --id KhronosGroup.VulkanSDK\n`
	+ `  Or download: ${VK_SDK_INSTALLER_URL}`);
	return false;
}

function whisperCppBuildDir(): string {
	return join(whisperCppDir(), 'build');
}

function tryBuild(sessionPath: string): boolean {
	const buildDir = whisperCppBuildDir();
	const vkDir = findVulkanSdkDir();
	const cmakeArgs = ['-B', buildDir, '-S', whisperCppDir(), '-DGGML_VULKAN=1'];

	// If SPIRV headers dir is available, pass explicit vars to help CMake find packages
	if (vkDir) {
		const spirvCmake = join(vkDir, 'Lib', 'cmake');
		cmakeArgs.push(`-DSPIRV-Headers_DIR=${spirvCmake}`);
		cmakeArgs.push(`-DCMAKE_PREFIX_PATH=${spirvCmake}`);
	}

	emitLog(sessionPath, `[Whisper] cmake configure (Vulkan) args=${cmakeArgs.join(' ')}`);
	const buildEnv: Record<string, string> = { ...process.env as Record<string, string> };
	if (vkDir) {
		buildEnv['VULKAN_SDK'] = vkDir;
	}
	const cfg = spawnSync('cmake', cmakeArgs, { timeout: 120_000, env: buildEnv });
	if (cfg.status !== 0) {
		const err = cfg.stderr?.toString() || '';
		emitLog(sessionPath, `[Whisper] cmake configure (Vulkan) failed:\n${err.slice(-2000)}`);
		return false;
	}

	emitLog(sessionPath, '[Whisper] cmake build (Vulkan)...');
	const build = spawnSync('cmake', ['--build', buildDir, '--config', 'Release', '-j', '4'],
		{ timeout: 600_000, env: buildEnv });
	if (build.status !== 0) {
		const err = build.stderr?.toString() || '';
		emitLog(sessionPath, `[Whisper] cmake build (Vulkan) failed:\n${err.slice(-2000)}`);
		return false;
	}

	const binPath = whisperCppBinaryPath();
	if (existsSync(binPath)) {
		emitLog(sessionPath, `[Whisper] Built ${binPath}`);
		return true;
	}
	emitLog(sessionPath, `[Whisper] Build completed but binary not found at ${binPath}`);
	return false;
}

function buildWhisperCpp(sessionPath: string): boolean {
	emitLog(sessionPath, '[Whisper] Starting Vulkan-enabled build flow: ensure SDK then build');
	if (!ensureVulkanSdk(sessionPath)) return false;
	return tryBuild(sessionPath);
}

export function ensureWhisperCppBinary(sessionPath: string): boolean {
	const binPath = whisperVulkanPath();
	if (existsSync(binPath)) return true;

	emitLog(sessionPath, `[Whisper] whisper-vulkan not found at ${binPath}`);

	// Attempt an automatic build once
	try {
		if (buildWhisperCpp(sessionPath)) {
			// re-evaluate path after build
			const newBin = whisperVulkanPath();
			if (existsSync(newBin)) {
				emitLog(sessionPath, `[Whisper] Found binary after build at ${newBin}`);
				return true;
			}
			// fallback: list build dir for diagnostics
			emitLog(sessionPath, `[Whisper] Build finished but binary still not found at ${newBin}`);
		} else {
			emitLog(sessionPath, '[Whisper] Automatic build attempt failed');
		}
	} catch (e:any) {
		emitLog(sessionPath, `[Whisper] Exception during automatic build: ${e?.message || e}`);
	}

	emitLog(sessionPath, '[Whisper] Build whisper.cpp with Vulkan (manual steps):\n'
		+ `  cd submodule/whisper.cpp\n`
		+ `  cmake -B build -DGGML_VULKAN=1\n`
		+ `  cmake --build build --config Release -j4\n`
		+ `  Or set WHISPER_MODEL to use a different runtime.`);
	return false;
}

export function ensureWhisperCpp(sessionPath: string): boolean {
	if (!ensureWhisperCppSubmodule(sessionPath)) return false;
	if (!ensureWhisperCppModel(sessionPath)) return false;
	if (!ensureWhisperCppBinary(sessionPath)) return false;
	return true;
}
