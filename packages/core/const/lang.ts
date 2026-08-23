export const langList = [
  "en",
  "zh",
  "vi", // 越南语
  "ja",
  "ko",
  "fr",
  "de",
  "es",
  "pt",
  "ru",
  "ar",
  "hi",
  "th",
  "id",
  "ms",
  "tl",
  "my",
  "km",
  "lo",
  "mn",
  "ne",
  "ur",
  "bn",
] as const;
export type TargetLang = (typeof langList)[number];
export type SourceLang = (typeof langList)[number];
