import { benchmarkGGML } from './ggml-cpu';
import { printSummary, saveResults } from './bench-shared';

const RESULTS_FILE = 'separate-ggml.json';

async function main() {
  console.log('=== Demucs ggml Benchmark ===\n');
  const results = await benchmarkGGML();
  printSummary(results);
  saveResults(results, RESULTS_FILE);
}

main().catch((err) => {
  console.error('Benchmark failed:', err);
  process.exit(1);
});
