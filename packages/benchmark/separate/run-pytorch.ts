import { readFileSync, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { benchmarkPyTorch } from './pytorch-cpu';
import { printSummary, RESULTS_DIR } from './bench-shared';

async function main() {
  const shifts = process.argv.includes('--shifts')
    ? Number(process.argv[process.argv.indexOf('--shifts') + 1])
    : 3;
  console.log(`=== Demucs PyTorch CPU Benchmark (shifts=${shifts}) ===\n`);
  const results = await benchmarkPyTorch(shifts);
  printSummary(results);

  const existingPath = join(RESULTS_DIR, 'separate-bench.json');
  let all: any[] = [];
  if (existsSync(existingPath)) {
    all = JSON.parse(readFileSync(existingPath, 'utf-8'));
  }
  all.push(...results);
  mkdirSync(RESULTS_DIR, { recursive: true });
  writeFileSync(existingPath, JSON.stringify(all, null, 2));
  console.log(`\nResults saved to ${existingPath}`);
}

main().catch((err) => {
  console.error('Benchmark failed:', err);
  process.exit(1);
});
