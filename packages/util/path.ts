export function getLastSegment(path: string) {
  const last = path.replace(/\\/g, "/").split("/").filter(Boolean).pop();
  if (!last) throw new Error(`Invalid path: ${path}`);
  return last;
}
