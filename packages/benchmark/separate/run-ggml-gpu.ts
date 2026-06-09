import { benchmarkGGMLGPU } from './ggml-gpu';
import { printSummary, saveResults } from './bench-shared';

const RESULTS_FILE = 'separate-ggml-gpu.json';

async function main() {
  console.log('=== Demucs ggml (GPU) Benchmark ===\n');
  console.log('Note: ggml path uses Eigen/CPU — no GPU backend exists.');
  console.log('This benchmark verifies CPU-only performance under the GGML label.\n');
  const results = await benchmarkGGMLGPU();
  printSummary(results);
  saveResults(results, RESULTS_FILE);
}

main().catch((err) => {
  console.error('Benchmark failed:', err);
  process.exit(1);
});
