import { test, expect } from 'bun:test';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';

const repoRoot = resolve(import.meta.dir, '../../../../..');
const srcDir = join(repoRoot, 'submodule', 'demucs.cpp');
const buildDir = join(srcDir, 'build');
const binPath = join(buildDir, 'demucs_mt.cpp.main');

test('demucs.cpp cmake build', async () => {
	if (existsSync(binPath)) {
		console.log(`  binary exists at ${binPath}, skipping build`);
		return;
	}

	console.log('  initializing git submodules...');
	const init = spawnSync('git', ['submodule', 'update', '--init', '--recursive'], {
		cwd: srcDir,
		stdio: 'inherit',
		timeout: 120_000,
	});
	expect(init.status).toBe(0);

	mkdirSync(buildDir, { recursive: true });

	console.log('  cmake configure...');
	const cfg = spawnSync('cmake', [
		'..', '-DCMAKE_BUILD_TYPE=Release', '-DCMAKE_POLICY_VERSION_MINIMUM=3.5',
	], { cwd: buildDir, stdio: 'inherit', timeout: 60_000 });
	expect(cfg.status).toBe(0);

	console.log('  cmake build...');
	const build = spawnSync('cmake', ['--build', '.', '--config', 'Release', '-j4'], {
		cwd: buildDir,
		stdio: 'inherit',
		timeout: 600_000,
	});
	expect(build.status).toBe(0);
	expect(existsSync(binPath)).toBe(true);

	console.log(`  done: ${binPath}`);
});
