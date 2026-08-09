export function nowISO(): string {
  return new Date().toISOString().replace(/\.\d{3}Z$/, "");
}

export function srtTime(ms: number, sep = ","): string {
  const total = Math.round(ms);
  const h = Math.floor(total / 3600000);
  const m = Math.floor((total % 3600000) / 60000);
  const s = Math.floor((total % 60000) / 1000);
  const ml = total % 1000;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}${sep}${String(ml).padStart(3, "0")}`;
}
