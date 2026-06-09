import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { benchmarkONNX } from './onnx-cpu';
import { benchmarkPyTorch } from './pytorch-cpu';
import { benchmarkGGML } from './ggml-cpu';
import { benchmarkGGMLGPU } from './ggml-gpu';
import { printSummary, RESULTS_DIR, type BenchmarkResult } from './bench-shared';

async function main() {
  const all: BenchmarkResult[] = [];

  console.log('=== Demucs Separation Benchmark ===\n');

  console.log('--- PyTorch CPU (shifts=3) ---');
  const pytorchResults3 = await benchmarkPyTorch(3);
  all.push(...pytorchResults3);

  console.log('\n--- PyTorch CPU (shifts=1, fair comparison) ---');
  const pytorchResults1 = await benchmarkPyTorch(1);
  all.push(...pytorchResults1);

  console.log('\n--- ONNX CPU ---');
  const onnxResults = await benchmarkONNX();
  all.push(...onnxResults);

  console.log('\n--- GGML CPU ---');
  const ggmlResults = await benchmarkGGML();
  all.push(...ggmlResults);

  console.log('\n--- GGML GPU (Eigen/CPU, no GPU backend) ---');
  const ggmlGpuResults = await benchmarkGGMLGPU();
  all.push(...ggmlGpuResults);

  printSummary(all);

  mkdirSync(RESULTS_DIR, { recursive: true });
  const path = join(RESULTS_DIR, 'separate-bench.json');
  writeFileSync(path, JSON.stringify(all, null, 2));
  console.log(`\nResults saved to ${path}`);
}

main().catch((err) => {
  console.error('Benchmark failed:', err);
  process.exit(1);
});
