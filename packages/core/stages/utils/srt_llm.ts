// srt_shared
export function parseLines(input: string, expectedCount: number): string[] | null {
  const texts: string[] = [];
  const lines = input.trim().split("\n");
  for (const line of lines) {
    const m = line.match(/^\s*\d+\s*[):.]\s*(.+)/);
    if (m) texts.push(m[1].trim());
  }
  if (texts.length !== expectedCount) return null;
  return texts;
}
