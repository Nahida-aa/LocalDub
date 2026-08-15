export function parseAsrOutput(stdout: string): string | null {
  for (const line of stdout.split("\n")) {
    const trimmed = line.trim();
    if (trimmed.startsWith("ASR_OUTPUT:")) return trimmed.slice("ASR_OUTPUT:".length).trim();
  }
  return stdout.trim() || null;
}
