import { getLocale, setLocale, locales } from "./paraglide/runtime.js";
import { m } from "./paraglide/messages.js";
export { getLocale, setLocale, locales, m, m as i18n };

type MessageKeys = keyof typeof m;

export const mD = <K extends MessageKeys>(key: K) => {
  const fn = m[key];
  if (typeof fn === "function") return fn;
  // 如果 存在前缀, 则只返回 结尾
  const keys = key.split("_");
  const fallbackText = keys.at(-1) || key;
  console.warn(`[i18n] 翻译丢失: ${key}，已回退显示: ${fallbackText}`);
  return () => fallbackText;
};

const localeNameMap = {
  en: m.en(),
  "zh-cn": m["zh-cn"](),
};
export type LocaleNameKey = keyof typeof localeNameMap;
export const tLocaleName = (locale: LocaleNameKey) => localeNameMap[locale];

const colorSchemeMap = {
  system: m.system(),
  light: m.light(),
  dark: m.dark(),
};
export type ColorSchemeKey = keyof typeof colorSchemeMap;
export const tColorScheme = (colorScheme: ColorSchemeKey) => colorSchemeMap[colorScheme];

const baseMap = {
  month: m.month,
  year: m.year,
};

export const tBase = (key: keyof typeof baseMap) => {
  return baseMap[key];
};

const statusMap = {
  pending: m.status_pending, // 等待中
  approved: m.status_approved, // 已通过
  rejected: m.status_rejected, // 已拒绝
  active: m.status_active, // 已激活
  expired: m.status_expired, // 已过期
  revoked: m.status_revoked, // 已撤销
};
export const tStatus = (status: keyof typeof statusMap) => {
  return statusMap[status];
};

const stepMap = {
  root: m.step_root,
  separate: m.step_separate,
  separate_after: m.step_separate_after,
  asr: m.step_asr,
  asr_ocr_pre: m.step_asr_ocr_pre,
  asr_ocr: m.step_asr_ocr,
  asr_ocr_fix: m.step_asr_ocr_fix,
  translate: m.step_translate,
};
export const tStep = (step: keyof typeof stepMap) => {
  return stepMap[step];
};
const identityTypeMap = {
  creator: m.identityType_creator,
  investor: m.identityType_investor,
  producer: m.identityType_producer,
  appreciator: m.identityType_appreciator,
};

export const tIdentityType = (identityType: keyof typeof identityTypeMap) => {
  return identityTypeMap[identityType]();
};

const loaderMap = {
  fabric: m.loader_fabric,
  neoforge: m.loader_neoforge,
  quilt: m.loader_quilt,
  babric: m.loader_babric,
  btababric: m.loader_btababric,
  bukkit: m.loader_bukkit,
  geyser_extension: m.loader_geyser_extension,
  folia: m.loader_folia,
  canvas: m.loader_canvas,
  sponge: m.loader_sponge,
  velocity: m.loader_velocity,
  waterfall: m.loader_waterfall,
  paper: m.loader_paper,
  datapack: m.loader_datapack,
  bungeecord: m.loader_bungeecord,
  javaagent: m.loader_javaagent,
  forge: m.loader_forge,
  iris: m.loader_iris,
  legacy_fabric: m.loader_legacy_fabric,
  liteloader: m.loader_liteloader,
  nilloader: m.loader_nilloader,
  optifine: m.loader_optifine,
  vanilla: m.loader_vanilla,
  ornithe: m.loader_ornithe,
  purpur: m.loader_purpur,
  resourcepack: m.loader_resourcepack,
  rift: m.loader_rift,
  risugami_s_modloader: m.loader_risugami_s_modloader,
  spigot: m.loader_spigot,
};
export const getLoaderName = (loader: keyof typeof loaderMap) => {
  return loaderMap[loader]();
};

const versionTypeMap = {
  alpha: m.versionType_alpha,
  beta: m.versionType_beta,
  release: m.versionType_release,
};
export const getVersionTypeName = (versionType: keyof typeof versionTypeMap) => {
  return versionTypeMap[versionType]();
};
