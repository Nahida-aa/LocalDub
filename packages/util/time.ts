export function nowISO(): string {
  return new Date().toISOString().replace(/\.\d{3}Z$/, "");
}
