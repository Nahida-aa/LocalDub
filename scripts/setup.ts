#!/usr/bin/env bun
/**
 * LocalDub 环境设置入口
 * 统一管理子模块初始化、依赖安装等
 */
import { join, resolve } from 'node:path';
import { existsSync, readFileSync } from 'node:fs';

const repoRoot = resolve(import.meta.dir, '..');
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

function checkPrerequisites() {
	const missing: string[] = [];
	for (const cmd of ['bun', 'ffmpeg', 'python']) {
		if (!Bun.which(cmd)) missing.push(cmd);
	}
	if (missing.length > 0) {
		log(`缺少命令: ${missing.join(', ')}`, 'red');
		process.exit(1);
	}
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

function checkEnv() {
	const envPath = join(repoRoot, '.env');
	const envExample = join(repoRoot, '.env.example');
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
	} else {
		log('跳过 pip 升级', 'gray');
	}

	// CUDA 时先安装 PyTorch CUDA 版本
	if (gpuMode === 'cuda') {
		const pytorchCu = join(repoRoot, 'requirements-pytorch-cu128.txt');
		if (existsSync(pytorchCu)) {
			log('安装 PyTorch CUDA 12.8...', 'yellow');
			run([pip, 'install', '-r', pytorchCu, '--quiet'], { cwd: repoRoot });
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
	};
}

function setupSubmodules(config: CliConfig, venv: string) {
	const sepRuntime = config.stages?.separate?.runtime;
	const ttsRuntime = config.stages?.tts?.runtime;

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
		}
	}

	// VoxCPM 依赖
	if (ttsRuntime === 'pytorch') {
		const voxcpmReq = join(repoRoot, 'submodule', 'VoxCPM', 'requirements.txt');
		if (existsSync(voxcpmReq)) {
			log('安装 VoxCPM 依赖...', 'yellow');
			run([pip, 'install', '-r', voxcpmReq, '--quiet'], { cwd: repoRoot });
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

	if (skipPipUpgrade) {
		log('将跳过 pip 升级', 'yellow');
	}

	checkPrerequisites();

	const gpuMode = detectGpu();
	log(`GPU 检测: ${gpuMode}`, 'cyan');

	// Read config.json
	const configPath = join(repoRoot, 'packages', 'cli', 'config.json');
	let config: CliConfig = {};
	if (existsSync(configPath)) {
		try {
			config = JSON.parse(readFileSync(configPath, 'utf-8'));
			log(`separate.runtime=${config.stages?.separate?.runtime}, tts.runtime=${config.stages?.tts?.runtime}`, 'cyan');
		} catch {
			log('config.json 解析失败', 'yellow');
		}
	} else {
		log('config.json not found, skipping submodule init', 'yellow');
	}

	checkEnv();

	const venv = join(repoRoot, '.venv');
	setupPython(venv, gpuMode, skipPipUpgrade);

	if (!skipJs) {
		const nodeModules = join(repoRoot, 'node_modules');
		setupJs(nodeModules);
	} else {
		log('跳过 JS 依赖安装', 'gray');
	}

	setupSubmodules(config, venv);

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
	console.log('');
}

main();
