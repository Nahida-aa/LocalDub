#!/usr/bin/env bun
/**
 * LocalDub 环境设置入口
 * 统一管理子模块初始化、依赖安装等
 */
import { join, resolve } from 'node:path';
import { existsSync, mkdirSync, readFileSync, readdirSync } from 'node:fs';

const repoRoot = resolve(import.meta.dir, '..', '..');
const isWindows = process.platform === 'win32';

// ---------------------------------------------------------------------------
// 工具函数
// ---------------------------------------------------------------------------

function log(msg: string, color: 'green' | 'yellow' | 'red' | 'cyan' | 'gray' = 'green') {
	const colors: Record<string, string> = {
		green: '\x1b[32m',
		yellow: '\x1b[33m',
		red: '\x1b[31m',
		cyan: '\x1b[36m',
		gray: '\x1b[90m',
	};
	console.log(`${colors[color]}[SETUP] ${msg}\x1b[0m`);
}

function run(cmd: string[], opts: { cwd?: string; env?: Record<string, string> } = {}): number {
	const r = Bun.spawnSync(cmd, {
		cwd: opts.cwd ?? repoRoot,
		stdout: 'inherit',
		stderr: 'inherit',
		env: { ...process.env, ...opts.env },
	});
	return r.exitCode ?? 0;
}

// ---------------------------------------------------------------------------
// 前置检测
// ---------------------------------------------------------------------------

function findFfmpegPath(): string | null {
	// 先检查 PATH 中是否存在
	if (Bun.which('ffmpeg')) {
		return 'ffmpeg';
	}

	// Windows: 使用 where.exe 查找
	if (isWindows) {
		const whereResult = Bun.spawnSync(['where.exe', 'ffmpeg'], {
			cwd: repoRoot,
		});
		if (whereResult.exitCode === 0 && whereResult.stdout) {
			const stdoutStr = whereResult.stdout.toString('utf-8');
			const paths = stdoutStr.trim().split('\n').filter(Boolean);
			if (paths.length > 0) {
				return paths[0].trim();
			}
		}

		// 检查 winget 常见安装路径
		const wingetPaths = [
			join(process.env.USERPROFILE || '', 'AppData', 'Local', 'Microsoft', 'WinGet', 'Packages', 'Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe'),
			join('C:', 'Program Files', 'FFmpeg'),
			join('C:', 'Program Files (x86)', 'FFmpeg'),
		];

		for (const basePath of wingetPaths) {
			if (existsSync(basePath)) {
				// 查找 ffmpeg-*-full_build/bin 目录
				const dirs = readdirSync(basePath);
				for (const dir of dirs) {
					if (dir.startsWith('ffmpeg-') && dir.endsWith('-full_build')) {
						const ffmpegPath = join(basePath, dir, 'bin', 'ffmpeg.exe');
						if (existsSync(ffmpegPath)) {
							return ffmpegPath;
						}
					}
				}
			}
		}
	}

	return null;
}

function updateEnvFile(key: string, value: string) {
	const envPath = join(repoRoot, '.env');
	if (!existsSync(envPath)) {
		log('.env 文件不存在', 'gray');
		return;
	}

	let content = readFileSync(envPath, 'utf-8');
	const regex = new RegExp(`^${key}=.*$`, 'm');
	
	if (regex.test(content)) {
		content = content.replace(regex, `${key}=${value}`);
	} else {
		content += `\n${key}=${value}`;
	}

	Bun.write(envPath, content);
	log(`已更新 .env 中 ${key}`, 'green');
}

function installFfmpeg(): boolean {
	if (!isWindows) {
		log('自动安装仅支持 Windows 系统', 'yellow');
		return false;
	}

	log('使用 winget 安装 Gyan.FFmpeg...', 'yellow');
	// 使用 powershell 执行 winget，因为 winget.exe 可能是符号链接
	const result = Bun.spawnSync(['powershell.exe', '-Command', 'winget install --id Gyan.FFmpeg --accept-source-agreements --accept-package-agreements'], {
		cwd: repoRoot,
		stdout: 'inherit',
		stderr: 'inherit',
	});

	if (result.exitCode === 0) {
		log('ffmpeg 安装成功', 'green');
		// 尝试重新查找路径并添加到 PATH
		const newPath = findFfmpegPath();
		if (newPath && newPath !== 'ffmpeg') {
			const ffmpegDir = resolve(newPath, '..');
			process.env.PATH = `${ffmpegDir};${process.env.PATH}`;
			log('已将 ffmpeg 路径添加到当前进程 PATH', 'green');
		}
		return true;
	} else {
		log(`ffmpeg 安装失败 (exit code: ${result.exitCode})`, 'red');
		return false;
	}
}

function checkPrerequisites() {
	let missing: string[] = [];
	let ffmpegPath = findFfmpegPath();

	if (!ffmpegPath) {
		missing.push('ffmpeg');
	}
	if (!Bun.which('bun')) {
		missing.push('bun');
	}
	if (!Bun.which('python')) {
		missing.push('python');
	}

	// 尝试自动安装 ffmpeg
	if (!ffmpegPath) {
		log('ffmpeg 未找到，尝试自动安装...', 'yellow');
		if (installFfmpeg()) {
			missing = missing.filter(m => m !== 'ffmpeg');
			ffmpegPath = findFfmpegPath();
		}
	}

	if (missing.length > 0) {
		log(`缺少命令: ${missing.join(', ')}`, 'red');
		log('请手动安装后再运行此脚本', 'yellow');
		process.exit(1);
	}

	// 如果 ffmpeg 在 PATH 中，直接显示成功
	if (ffmpegPath === 'ffmpeg') {
		log('bun / ffmpeg / python 均已安装', 'green');
		return;
	}

	// 如果 ffmpeg 不在 PATH 但找到了，写入 .env 并临时添加到进程 PATH
	log(`ffmpeg 已安装但不在 PATH 中，找到路径: ${ffmpegPath}`, 'yellow');
	updateEnvFile('FFMPEG_PATH', ffmpegPath!);
	const ffmpegDir = resolve(ffmpegPath!, '..');
	process.env.PATH = `${ffmpegDir}${isWindows ? ';' : ':'}${process.env.PATH}`;
	log('已将 ffmpeg 路径添加到当前进程 PATH', 'green');
	log('bun / ffmpeg / python 均已安装', 'green');
}

// ---------------------------------------------------------------------------
// GPU 检测
// ---------------------------------------------------------------------------

function detectGpu(): 'cuda' | 'cpu' {
	if (!isWindows) {
		const nvidia = run(['nvidia-smi'], { cwd: repoRoot });
		if (nvidia === 0) return 'cuda';
		return 'cpu';
	}
	// Windows: try nvidia-smi.exe
	const r = Bun.spawnSync(['nvidia-smi.exe'], { cwd: repoRoot });
	return r.exitCode === 0 ? 'cuda' : 'cpu';
}

// ---------------------------------------------------------------------------
// .env 检查
// ---------------------------------------------------------------------------
const envPath = join(repoRoot, '.env');
const envExample = join(repoRoot, '.env.example');
function checkEnv() {
	if (!existsSync(envPath) && existsSync(envExample)) {
		// Bun doesn't have cp, use spawn
		run(['cmd', '/c', 'copy', '.env.example', '.env'], { cwd: repoRoot });
		log('已创建 .env（请按需编辑）', 'yellow');
	} else {
		log('.env 已存在', 'gray');
	}
}

// ---------------------------------------------------------------------------
// Python 虚拟环境
// ---------------------------------------------------------------------------

function setupPython(venv: string, gpuMode: 'cuda' | 'cpu', skipPipUpgrade: boolean = false) {
	if (!existsSync(venv)) {
		log('创建虚拟环境...', 'yellow');
		run(['python', '-m', 'venv', venv], { cwd: repoRoot });
	}

	const pip = isWindows
		? join(venv, 'Scripts', 'pip.exe')
		: join(venv, 'bin', 'pip');

	if (!skipPipUpgrade) {
		log('升级 pip...', 'gray');
		const python = isWindows
			? join(venv, 'Scripts', 'python.exe')
			: join(venv, 'bin', 'python');
		run([python, '-m', 'pip', 'install', '--quiet', '--upgrade', 'pip'], { cwd: repoRoot });
		log('pip 升级完成', 'gray');
	} else {
		log('跳过 pip 升级', 'gray');
	}

	// CUDA 时先安装 PyTorch CUDA 版本
	if (gpuMode === 'cuda') {
		const pytorchCu = join(repoRoot, 'requirements-pytorch-cu128.txt');
		if (existsSync(pytorchCu)) {
			log('安装 PyTorch CUDA 12.8...', 'yellow');
			run([pip, 'install', '-r', pytorchCu, '--quiet'], { cwd: repoRoot });
			log('PyTorch CUDA 12.8 安装完成', 'gray');
		}
	}

	// 安装其他依赖（CPU 时指定 index URL）
	const torchIndex = gpuMode === 'cpu'
		? ['--index-url', 'https://download.pytorch.org/whl/cpu']
		: [];

	log(`安装 Python 依赖 (${gpuMode})...`, 'yellow');
	const args = [pip, 'install', '-r', join(repoRoot, 'requirements.txt'), '--quiet'];
	if (torchIndex.length) args.push(...torchIndex);
	run(args, { cwd: repoRoot });
	log('Python 依赖完成', 'green');
}

// ---------------------------------------------------------------------------
// JS 依赖
// ---------------------------------------------------------------------------

function setupJs(nodeModules: string) {
	if (!existsSync(nodeModules)) {
		log('bun install...', 'yellow');
		run(['bun', 'install'], { cwd: repoRoot });
	} else {
		log('node_modules 已存在', 'gray');
	}
}

// ---------------------------------------------------------------------------
// 子模块初始化（根据 config.json）
// ---------------------------------------------------------------------------

interface CliConfig {
	stages?: {
		separate?: { runtime?: string };
		tts?: { runtime?: string };
		asr_ocr?: { runtime?: string };
	};
}

function setupSubmodules(config: CliConfig, venv: string) {
	const sepRuntime = config.stages?.separate?.runtime;
	const ttsRuntime = config.stages?.tts?.runtime;
	const ocrRuntime = config.stages?.asr_ocr?.runtime;

	const needed: string[] = [];
	if (sepRuntime === 'pytorch' || sepRuntime === 'ggml') {
		needed.push('submodule/demucs');
	}
	if (sepRuntime === 'ggml') {
		needed.push('submodule/demucs.cpp');
	}
	if (ttsRuntime === 'pytorch') {
		needed.push('submodule/VoxCPM');
	}

	if (needed.length === 0) {
		log('No submodules needed based on config', 'gray');
		return;
	}

	const unique = [...new Set(needed)];
	for (const sm of unique) {
		const smPath = join(repoRoot, sm.replace('/', isWindows ? '\\' : '/'));
		const gitDir = join(smPath, '.git');
		if (existsSync(gitDir)) {
			log(`${sm} already initialized`, 'gray');
		} else {
			log(`Initializing ${sm}...`, 'yellow');
			const code = run(['git', 'submodule', 'update', '--init', sm], { cwd: repoRoot });
			if (code !== 0) {
				log(`Failed to init ${sm}`, 'red');
			}
		}
	}

	// 安装子模块依赖
	const python = isWindows
		? join(venv, 'Scripts', 'python.exe')
		: join(venv, 'bin', 'python');
	const pip = isWindows
		? join(venv, 'Scripts', 'pip.exe')
		: join(venv, 'bin', 'pip');

	// demucs 核心依赖（通过 requirements-demucs.txt）
	if (sepRuntime === 'pytorch' || sepRuntime === 'ggml') {
		const demucsReq = join(repoRoot, 'requirements-demucs.txt');
		if (existsSync(demucsReq)) {
			log('安装 demucs 核心依赖...', 'yellow');
			run([pip, 'install', '-r', demucsReq, '--quiet'], { cwd: repoRoot });
			log('demucs 核心依赖安装完成', 'gray');
		}
	}

	// VoxCPM 依赖
	if (ttsRuntime === 'pytorch') {
		const voxcpmReq = join(repoRoot, 'requirements-voxcpm.txt');
		if (existsSync(voxcpmReq)) {
			log('安装 VoxCPM 依赖...', 'yellow');
			run([pip, 'install', '-r', voxcpmReq, '--quiet'], { cwd: repoRoot });
			log('VoxCPM 依赖安装完成', 'gray');
		}
	}

	// GGML binary check
	if (sepRuntime === 'ggml') {
		const ggmlBin = join(repoRoot, 'submodule', 'demucs.cpp', 'build', 'demucs_mt.cpp.main.exe');
		if (existsSync(ggmlBin)) {
			log(`GGML binary exists`, 'gray');
		} else {
			log('GGML binary not found, will build at runtime', 'yellow');
		}
	}
}

// ---------------------------------------------------------------------------
// C++ OCR 编译
// ---------------------------------------------------------------------------

function checkCommand(cmd: string): boolean {
	return Bun.which(cmd) !== null;
}

/**
 * 检查 OpenCV 是否已安装
 */
function checkOpenCV(): boolean {
	if (!isWindows) {
		// Linux: 检查 pkg-config 或常见路径
		if (run(['pkg-config', '--exists', 'opencv4'], { cwd: repoRoot }) === 0) {
			return true;
		}
		if (existsSync('/usr/lib/libopencv_core.so')) {
			return true;
		}
		return false;
	}

	// Windows: 优先检查 MSYS2 pacman 安装的 OpenCV
	const msys2Dir = 'C:\\msys64';
	const msys2OpenCv = join(msys2Dir, 'mingw64');
	if (existsSync(join(msys2OpenCv, 'include', 'opencv2', 'core.hpp')) ||
		existsSync(join(msys2OpenCv, 'lib', 'libopencv_core.a')) ||
		existsSync(join(msys2OpenCv, 'bin', 'opencv_world*.dll'))) {
		process.env.OpenCV_DIR = msys2OpenCv;
		return true;
	}

	// 检查 vcpkg 安装的 OpenCV
	const vcpkgDir = join(repoRoot, 'submodule', 'vcpkg', 'installed', 'x64-windows');
	if (existsSync(join(vcpkgDir, 'include', 'opencv2', 'core.hpp'))) {
		process.env.OpenCV_DIR = vcpkgDir;
		return true;
	}

	// 检查常见安装路径
	const paths = [
		'C:\\opencv\\build',
		'C:\\opencv4\\build',
		join(process.env.ProgramFiles || 'C:\\Program Files', 'OpenCV', 'build'),
		join(process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)', 'OpenCV', 'build'),
	];

	for (const path of paths) {
		if (existsSync(join(path, 'include', 'opencv2', 'core.hpp'))) {
			process.env.OpenCV_DIR = path;
			return true;
		}
	}

	// 检查环境变量
	if (process.env.OpenCV_DIR && existsSync(process.env.OpenCV_DIR)) {
		return true;
	}

	return false;
}

/**
 * 使用子模块安装 vcpkg 并构建 OpenCV
 */
function installOpenCVWithVcpkgSubmodule(): boolean {
	const vcpkgSubmodule = join(repoRoot, 'submodule', 'vcpkg');
	
	// 检查子模块是否已初始化
	if (!existsSync(vcpkgSubmodule) || !existsSync(join(vcpkgSubmodule, 'CMakeLists.txt'))) {
		log('vcpkg 子模块未初始化，正在添加...', 'yellow');
		
		// 添加 vcpkg 子模块
		const addResult = run(['git', 'submodule', 'add', '-f', 'https://github.com/microsoft/vcpkg.git', 'submodule/vcpkg'], { cwd: repoRoot });
		if (addResult !== 0) {
			log('vcpkg 子模块添加失败', 'red');
			return false;
		}
	}
	
	// 检查 vcpkg 是否已 bootstrap
	const vcpkgExe = join(vcpkgSubmodule, 'vcpkg.exe');
	if (!existsSync(vcpkgExe)) {
		log('Bootstrap vcpkg...', 'yellow');
		
		// 添加 MSYS2 到 PATH 以便 bootstrap 找到编译器
		const msys2Path = 'C:\\msys64\\mingw64\\bin;C:\\msys64\\usr\\bin';
		const oldPath = process.env.PATH;
		process.env.PATH = msys2Path + ';' + oldPath;
		
		// 运行 bootstrap-vcpkg.bat
		const bootstrapResult = run(['cmd.exe', '/c', 'bootstrap-vcpkg.bat'], { cwd: vcpkgSubmodule });
		
		process.env.PATH = oldPath;
		
		if (bootstrapResult !== 0) {
			log('vcpkg bootstrap 失败', 'red');
			return false;
		}
	}
	
	log(`找到 vcpkg: ${vcpkgExe}`, 'gray');
	
	// 设置环境变量
	const vcpkgDir = vcpkgSubmodule;
	
	// 检查是否已经安装了 OpenCV
	const opencvDir = join(vcpkgDir, 'installed', 'x64-windows');
	const opencvLib = join(opencvDir, 'lib', 'libopencv_core.a');
	const opencvDll = join(opencvDir, 'bin', 'opencv_world481.dll');
	if (existsSync(opencvLib) || existsSync(opencvDll)) {
		log('OpenCV 已通过 vcpkg 安装', 'gray');
		process.env.OpenCV_DIR = opencvDir;
		return true;
	}
	
	// 使用 vcpkg 安装 opencv4
	log('使用 vcpkg 安装 opencv4:x64-windows...', 'yellow');
	
	// 设置 VCPKG_ROOT
	const oldVcpkgRoot = process.env.VCPKG_ROOT;
	process.env.VCPKG_ROOT = vcpkgDir;
	
	// 添加 vcpkg 到 PATH
	const oldPath = process.env.PATH;
	process.env.PATH = vcpkgDir + ';' + oldPath;
	
	const installResult = run([vcpkgExe, 'install', 'opencv4:x64-windows'], { cwd: repoRoot });
	
	process.env.PATH = oldPath;
	if (oldVcpkgRoot) process.env.VCPKG_ROOT = oldVcpkgRoot;
	else delete process.env.VCPKG_ROOT;
	
	if (installResult !== 0) {
		log('vcpkg install opencv4 失败', 'red');
		return false;
	}
	
	log('OpenCV 通过 vcpkg 安装成功', 'green');
	
	// 设置 OpenCV_DIR
	if (existsSync(opencvDir)) {
		process.env.OpenCV_DIR = opencvDir;
		log(`设置 OpenCV_DIR=${opencvDir}`, 'green');
	}
	
	return true;
}

/**
 * 使用子模块构建 OpenCV
 */
function buildOpenCVFromSubmodule(): boolean {
	const opencvSubmodule = join(repoRoot, 'submodule', 'opencv');
	const opencvBuildDir = join(opencvSubmodule, 'build');
	
	// 检查子模块是否已初始化
	if (!existsSync(opencvSubmodule) || !existsSync(join(opencvSubmodule, 'CMakeLists.txt'))) {
		log('OpenCV 子模块未初始化，正在添加...', 'yellow');
		
		// 先尝试 git submodule add（如果 .gitmodules 中没有配置）
		const addResult = run(['git', 'submodule', 'add', '-f', 'https://github.com/opencv/opencv.git', 'submodule/opencv'], { cwd: repoRoot });
		if (addResult !== 0) {
			// 如果 add 失败，尝试 update --init
			log('git submodule add 失败，尝试 update --init...', 'gray');
			const initResult = run(['git', 'submodule', 'update', '--init', '--recursive', 'submodule/opencv'], { cwd: repoRoot });
			if (initResult !== 0) {
				log('OpenCV 子模块初始化失败', 'red');
				return false;
			}
		}
	}
	
	// 检查是否已构建
	const opencvInstallDir = join(opencvBuildDir, 'install');
	if (existsSync(join(opencvInstallDir, 'include', 'opencv2', 'core.hpp'))) {
		log(`OpenCV 已构建: ${opencvInstallDir}`, 'gray');
		process.env.OpenCV_DIR = opencvInstallDir;
		return true;
	}
	
	// 构建 OpenCV
	log('构建 OpenCV（这可能需要 30-60 分钟）...', 'yellow');
	
	// 创建 build 目录
	if (!existsSync(opencvBuildDir)) {
		mkdirSync(opencvBuildDir, { recursive: true });
	}
	
	// cmake configure
	const cmakeConfig = isWindows
		? ['cmake', '-B', opencvBuildDir, '-S', opencvSubmodule, '-G', 'MinGW Makefiles', '-DCMAKE_BUILD_TYPE=Release', '-DCMAKE_INSTALL_PREFIX=' + join(opencvBuildDir, 'install'), '-DBUILD_SHARED_LIBS=OFF', '-DBUILD_TESTS=OFF', '-DBUILD_PERF_TESTS=OFF', '-DBUILD_EXAMPLES=OFF', '-DBUILD_opencv_apps=OFF']
		: ['cmake', '-B', opencvBuildDir, '-S', opencvSubmodule, '-DCMAKE_BUILD_TYPE=Release', '-DCMAKE_INSTALL_PREFIX=' + join(opencvBuildDir, 'install'), '-DBUILD_SHARED_LIBS=OFF', '-DBUILD_TESTS=OFF', '-DBUILD_PERF_TESTS=OFF', '-DBUILD_EXAMPLES=OFF', '-DBUILD_opencv_apps=OFF'];
	
	const configResult = run(cmakeConfig, { cwd: repoRoot });
	if (configResult !== 0) {
		log('OpenCV cmake configure 失败', 'red');
		return false;
	}
	
	// cmake build
	const buildResult = run(['cmake', '--build', opencvBuildDir, '--config', 'Release', '--target', 'install', '-j', '4'], { cwd: repoRoot });
	if (buildResult !== 0) {
		log('OpenCV cmake build 失败', 'red');
		return false;
	}
	
	log('OpenCV 构建成功', 'green');
	process.env.OpenCV_DIR = opencvInstallDir;
	return true;
}

/**
 * 使用 vcpkg 安装 OpenCV
 */
function installOpenCVWithVcpkg(): boolean {
	// 检查 vcpkg 是否已安装
	let vcpkgPath = Bun.which('vcpkg');
	if (!vcpkgPath) {
		// 尝试在常见位置查找
		const possiblePaths = [
			'C:\\vcpkg\\vcpkg.exe',
			join(process.env.PROGRAMFILES || 'C:\\Program Files', 'vcpkg', 'vcpkg.exe'),
			join(process.env.LOCALAPPDATA || 'C:\\Users\\' + (process.env.USERNAME || '') + '\\AppData\\Local', 'vcpkg', 'vcpkg.exe'),
		];
		for (const path of possiblePaths) {
			if (existsSync(path)) {
				vcpkgPath = path;
				break;
			}
		}
	}
	
	if (!vcpkgPath) {
		log('vcpkg 未安装', 'yellow');
		return false;
	}
	
	log(`找到 vcpkg: ${vcpkgPath}`, 'gray');
	
	// 使用 vcpkg 安装 opencv4
	log('执行 vcpkg install opencv4...', 'yellow');
	const installResult = Bun.spawnSync([vcpkgPath, 'install', 'opencv4'], {
		cwd: repoRoot,
		stdout: 'inherit',
		stderr: 'inherit',
	});
	
	if (installResult.exitCode === 0) {
		log('OpenCV 通过 vcpkg 安装成功', 'green');
		
		// 设置 OpenCV_DIR 环境变量
		const vcpkgDir = resolve(vcpkgPath, '..');
		const opencvDir = join(vcpkgDir, 'installed', 'x64-windows');
		
		if (existsSync(opencvDir)) {
			process.env.OpenCV_DIR = opencvDir;
			log(`设置 OpenCV_DIR=${opencvDir}`, 'green');
		}
		
		return true;
	} else {
		log(`vcpkg install opencv4 失败 (exit code: ${installResult.exitCode})`, 'red');
		return false;
	}
}

/**
 * 使用 MSYS2 pacman 安装 OpenCV
 */
function installOpenCVWithPacman(): boolean {
	// 检查 MSYS2 是否已安装
	const msys2Dir = 'C:\\msys64';
	if (!existsSync(msys2Dir)) {
		log('MSYS2 未安装，跳过', 'gray');
		return false;
	}

	// 检查 pacman 是否可用
	const bashBin = join(msys2Dir, 'usr', 'bin', 'bash.exe');
	if (!existsSync(bashBin)) {
		log('bash 未找到，跳过', 'gray');
		return false;
	}

	// 检查 OpenCV 是否已安装
	const opencvLib = join(msys2Dir, 'mingw64', 'lib', 'libopencv_core.a');
	const opencvDll = join(msys2Dir, 'mingw64', 'bin', 'opencv_world490.dll');
	if (existsSync(opencvLib) || existsSync(opencvDll)) {
		log('OpenCV 已通过 pacman 安装', 'gray');
		process.env.OpenCV_DIR = join(msys2Dir, 'mingw64');
		return true;
	}

	// 使用 pacman 安装 opencv
	log('使用 pacman 安装 opencv（下载约 315 MB，安装约 1.8 GB）...', 'yellow');

	// 使用 bash -lc 执行命令（避免 PowerShell 引号问题）
	const pacmanCmd = 'rm -f /var/lib/pacman/db.lck && pacman -S --noconfirm --overwrite "*" mingw-w64-x86_64-opencv';
	const result = Bun.spawnSync([
		bashBin, '-lc', pacmanCmd
	], {
		cwd: repoRoot,
		stdout: 'inherit',
		stderr: 'inherit',
	});

	if (result.exitCode !== 0) {
		log('pacman 安装 opencv 失败', 'red');
		return false;
	}

	log('OpenCV 通过 pacman 安装成功', 'green');
	process.env.OpenCV_DIR = join(msys2Dir, 'mingw64');
	return true;
}

/**
 * 自动安装 OpenCV（Windows）
 */
function installOpenCV(): boolean {
	if (!isWindows) {
		log('OpenCV 自动安装仅支持 Windows', 'yellow');
		return false;
	}

	// 优先尝试使用 MSYS2 pacman 安装
	log('尝试使用 MSYS2 pacman 安装 OpenCV...', 'yellow');
	if (installOpenCVWithPacman()) {
		return true;
	}

	// pacman 方案失败，尝试使用 vcpkg 子模块安装
	log('pacman 方案失败，尝试使用 vcpkg 子模块...', 'yellow');
	if (installOpenCVWithVcpkgSubmodule()) {
		return true;
	}

	// vcpkg 子模块方案失败，尝试直接构建 OpenCV
	log('vcpkg 子模块方案失败，尝试构建 OpenCV...', 'yellow');
	if (buildOpenCVFromSubmodule()) {
		return true;
	}

	// 所有方法都失败，给出手动安装提示
	log('', 'yellow');
	log('┌────────────────────────────────────────┐', 'yellow');
	log('│ 需要安装 OpenCV 以编译 ort-cpp  │', 'yellow');
	log('└────────────────────────────────────────┘', 'yellow');
	log('', 'yellow');
	log('手动安装 OpenCV:', 'yellow');
	log('', 'yellow');
	log('  - 使用 pacman (推荐):', 'yellow');
	log('    pacman -S mingw-w64-x86_64-opencv', 'yellow');
	log('', 'yellow');
	log('  - 使用 vcpkg:', 'yellow');
	log('    git clone https://github.com/microsoft/vcpkg', 'yellow');
	log('    cd vcpkg && bootstrap-vcpkg.bat', 'yellow');
	log('    vcpkg install opencv4:x64-windows', 'yellow');
	log('', 'yellow');
	log('  - 或下载预构建版本:', 'yellow');
	log('    https://github.com/opencv/opencv/releases', 'yellow');
	log('    解压后设置环境变量 OpenCV_DIR', 'yellow');
	return false;
}

/**
 * 编译 OCR C++ 二进制 (ort-cpp)
 * @param project - 项目目录名 (目前只有 'ort-cpp')
 * @param binaryName - 二进制名 'subtitle_ocr_ort_cpp'
 */
function setupOcrCpp(project: 'ort-cpp', binaryName: 'subtitle_ocr_ort_cpp') {
	const ortBase = join(repoRoot, 'packages', 'tmp', isWindows ? 'onnxruntime-win-x64-1.26.0' : 'onnxruntime-linux-x64-1.24.4');
	const ortExtDir = join(ortBase, isWindows ? 'onnxruntime-win-x64-1.26.0' : 'onnxruntime-linux-x64-1.24.4');
	const cppSourceDir = join(repoRoot, 'packages', 'subtitle-ocr', project);
	const cppBuildDir = join(cppSourceDir, 'build');
	const ocrBinary = join(cppBuildDir, isWindows ? `${binaryName}.exe` : binaryName);

	// 若二进制已存在，跳过
	if (existsSync(ocrBinary)) {
		log(`${binaryName} 已编译`, 'gray');
		return;
	}

	// ort-cpp 需要 OpenCV，先检查并安装
	if (project === 'ort-cpp') {
		// 优先使用 MSYS2 pacman 安装的 OpenCV
		const msys2Dir = 'C:\\msys64';
		const msys2OpenCv = join(msys2Dir, 'mingw64');
		if (existsSync(join(msys2OpenCv, 'lib', 'libopencv_core.a')) ||
			existsSync(join(msys2OpenCv, 'bin', 'opencv_world490.dll'))) {
			log(`OpenCV 已安装 (MSYS2): ${msys2OpenCv}`, 'gray');
			process.env.OpenCV_DIR = msys2OpenCv;
		} else if (!checkOpenCV()) {
			log('OpenCV 未安装，尝试自动安装...', 'yellow');
			if (!installOpenCV()) {
				return;
			}
		} else {
			log(`OpenCV 已安装 (OpenCV_DIR=${process.env.OpenCV_DIR})`, 'gray');
		}
	}

	// 下载 onnxruntime
	if (!existsSync(ortExtDir)) {
		log('下载 onnxruntime...', 'yellow');
		const tmpDir = join(repoRoot, 'packages', 'tmp');
		if (!existsSync(tmpDir)) {
			run(isWindows
				? ['cmd', '/c', 'mkdir', tmpDir]
				: ['mkdir', '-p', tmpDir],
				{ cwd: repoRoot });
		}

		const archivename = isWindows ? 'onnxruntime-win-x64-1.26.0.zip' : 'onnxruntime-linux-x64-1.24.4.tgz';
		const archivePath = join(tmpDir, archivename);
		const downloadUrl = isWindows
			? `https://github.com/microsoft/onnxruntime/releases/download/v1.26.0/${archivename}`
			: `https://github.com/microsoft/onnxruntime/releases/download/v1.24.4/${archivename}`;

		// 使用 curl 下载
		const curlCode = run(
			['curl', '-L', '-o', archivePath, downloadUrl],
			{ cwd: repoRoot }
		);
		if (curlCode !== 0) {
			log(`onnxruntime 下载失败 (curl exit ${curlCode})`, 'red');
			return;
		}

		// 解压
		log('解压 onnxruntime...', 'yellow');
		if (isWindows) {
			run(['powershell', '-Command', `Expand-Archive -Path "${archivePath}" -DestinationPath "${tmpDir}" -Force`], { cwd: repoRoot });
		} else {
			run(['tar', '-xzf', archivePath, '-C', tmpDir], { cwd: repoRoot });
		}
	} else {
		log('onnxruntime 已存在', 'gray');
	}

	// 检查 cmake
	if (!checkCommand('cmake')) {
		log(`cmake 未找到，跳过 ${binaryName} 编译`, 'yellow');
		return;
	}

	// 添加 MSYS2 到 PATH
	const msys2Dir = 'C:\\msys64';
	const useMsys2 = isWindows;
	const msys2Paths = isWindows
		? join(msys2Dir, 'mingw64', 'bin') + ';' + join(msys2Dir, 'usr', 'bin') + ';'
		: '';

	// 使用 MSYS2 原生环境
	const opencvDir = useMsys2
		? join(msys2Dir, 'mingw64')
		: (process.env.OpenCV_DIR || '');

	const buildEnv: Record<string, string> = {
		...process.env,
		PATH: `${msys2Paths}${process.env.PATH}`,
		OpenCV_DIR: opencvDir,
	};

	// 编译
	log(`编译 ${binaryName}...`, 'yellow');

	// 构建 cmake 命令
	const cmakeBaseArgs = [
		'cmake',
		'-B', cppBuildDir,
		'-S', join(repoRoot, 'packages', 'subtitle-ocr', project),
		`-DORT_DIR=${ortExtDir}`,
	];

	const cmakeFinalArgs = [
		...cmakeBaseArgs,
		...(isWindows ? ['-G', 'MinGW Makefiles'] : [])
	];

	let cmakeCode: number;

	if (useMsys2) {
		// 使用 PowerShell 执行 cmake，但确保 MSYS2 在 PATH 中
		const cmakeCmd = [...cmakeFinalArgs].map(arg => `"${arg}"`).join(' ');
		const psCmd = `$env:PATH = 'C:\\msys64\\mingw64\\bin;C:\\msys64\\usr\\bin;' + $env:PATH; cd '${repoRoot}'; ${cmakeCmd}; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }`;
		const cmakeBashResult = Bun.spawnSync([
			'powershell.exe', '-Command', psCmd
		], {
			cwd: repoRoot,
			env: buildEnv,
			stdout: 'inherit',
			stderr: 'inherit',
		});
		cmakeCode = cmakeBashResult.exitCode;
	} else {
		cmakeCode = run(cmakeFinalArgs, { cwd: repoRoot, env: buildEnv });
	}

	if (cmakeCode !== 0) {
		log(`cmake configure 失败 (exit ${cmakeCode})`, 'red');
		return;
	}

	// 编译
	let buildCode: number;
	if (useMsys2) {
		const buildCmd = `$env:PATH = 'C:\\msys64\\mingw64\\bin;C:\\msys64\\usr\\bin;' + $env:PATH; cd '${repoRoot}'; cmake --build '${cppBuildDir}' --parallel`;
		const buildBashResult = Bun.spawnSync([
			'powershell.exe', '-Command', buildCmd
		], {
			cwd: repoRoot,
			env: buildEnv,
			stdout: 'inherit',
			stderr: 'inherit',
		});
		buildCode = buildBashResult.exitCode;
	} else {
		buildCode = run(['cmake', '--build', cppBuildDir, '--parallel'], { cwd: repoRoot, env: buildEnv });
	}

	if (buildCode !== 0) {
		log(`${binaryName} 编译失败 (exit ${buildCode})`, 'red');
		return;
	}

	if (existsSync(ocrBinary)) {
		log(`${binaryName} 编译成功`, 'green');
	} else {
		log(`编译完成但 binary 未找到: ${ocrBinary}`, 'yellow');
	}
}

// ---------------------------------------------------------------------------
// 工作目录
// ---------------------------------------------------------------------------

function setupWorkdir(envWorkfolder?: string) {
	const workfolder = envWorkfolder ?? join(repoRoot, 'workfolder');
	// Use mkdir -p via spawn
	if (isWindows) {
		run(['cmd', '/c', 'mkdir', workfolder], { cwd: repoRoot });
	} else {
		run(['mkdir', '-p', workfolder], { cwd: repoRoot });
	}
	log(`工作目录: ${workfolder}`, 'green');
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function main() {
	console.log('\n=== LocalDub 环境设置 ===\n');

	const args = process.argv.slice(2);
	const skipPipUpgrade = args.includes('--skip-pip-upgrade');
	const skipJs = args.includes('--skip-js');
	const skipOcr = args.includes('--skip-ocr');

	if (skipPipUpgrade) {
		log('将跳过 pip 升级', 'yellow');
	}
	checkEnv();
	checkPrerequisites();

	const gpuMode = detectGpu();
	log(`GPU 检测: ${gpuMode}`, 'cyan');

	// Read config.json
	const configPath = join(repoRoot, 'packages', 'cli', 'config.json');
	let config: CliConfig = {};
	if (existsSync(configPath)) {
		try {
			config = JSON.parse(readFileSync(configPath, 'utf-8'));
			log(`separate.runtime=${config.stages?.separate?.runtime}, tts.runtime=${config.stages?.tts?.runtime}, asr_ocr.runtime=${config.stages?.asr_ocr?.runtime}`, 'cyan');
		} catch {
			log('config.json 解析失败', 'yellow');
		}
	} else {
		log('config.json not found, skipping submodule init', 'yellow');
	}

	const venv = join(repoRoot, '.venv');
	setupPython(venv, gpuMode, skipPipUpgrade);

	if (!skipJs) {
		const nodeModules = join(repoRoot, 'node_modules');
		setupJs(nodeModules);
	} else {
		log('跳过 JS 依赖安装', 'gray');
	}

	setupSubmodules(config, venv);

	// OCR C++ 编译：根据 runtime 选择编译目标
	const ocrRuntime = config.stages?.asr_ocr?.runtime;

	if (!skipOcr) {
		if (ocrRuntime === 'ort-cpp') {
			setupOcrCpp('ort-cpp', 'subtitle_ocr_ort_cpp');
		} else {
			log(`asr_ocr.runtime=${ocrRuntime}，跳过 OCR C++ 编译`, 'gray');
		}
	} else {
		log('跳过 OCR 编译 (--skip-ocr)', 'gray');
	}

	const envWorkfolder = process.env['WORKFOLDER'];
	setupWorkdir(envWorkfolder);

	console.log('\n=== 安装完成 ===');
	console.log('\n使用方式:');
	console.log('  编辑 .env 中的 API key 和配置');
	console.log('  编辑 packages/cli/config.json 中的任务配置');
	console.log('  运行: cd packages/cli && bun run run-task.ts');
	console.log('\n可选参数:');
	console.log('  --skip-pip-upgrade   跳过 pip 升级');
	console.log('  --skip-js            跳过 JS 依赖安装');
	console.log('  --skip-ocr           跳过 OCR binary 编译');
	console.log('');
}

main();
