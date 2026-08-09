import { allChecks, ensureFns } from "./items";
import { envDescribeMap, envList } from "./input";
import type { CheckResult } from "../../../env/types";
import type { EnvName } from "./input";
import { t, locale, type ServerI18nKey } from "@repo/shared/i18n/server";

function resolveTargets(targets?: string[]): string[] {
  if (!targets || targets.length === 0) return envList as string[];
  const valid = targets.filter((t) => t in allChecks);
  if (valid.length === 0) return envList as string[];
  return valid;
}

export async function runCheck(targets?: string[]): Promise<CheckResult[]> {
  const selected = resolveTargets(targets);
  const results: CheckResult[] = [];

  for (const key of selected) {
    const fn = allChecks[key];
    if (!fn) {
      results.push({ key, status: "skip", data: {}, required: false });
      continue;
    }
    try {
      const r = await fn();
      results.push(r);
    } catch {
      results.push({ key, status: "fail", data: {}, required: false });
    }
  }

  return results;
}

export async function runEnsure(targets?: string[]): Promise<CheckResult[]> {
  const selected = resolveTargets(targets);
  const results: CheckResult[] = [];

  for (const key of selected) {
    const fn = ensureFns[key];
    if (!fn) {
      results.push({
        key,
        status: "skip",
        data: {},
        required: false,
      });
      continue;
    }
    try {
      const r = await fn();
      results.push(r);
    } catch {
      results.push({ key, status: "fail", data: {}, required: false });
    }
  }

  return results;
}

export function formatResult(r: CheckResult): string {
  const key =
    r.key === "vcpkg" && r.status === "fail" && r.data.kind
      ? `env_vcpkg_fail_${r.data.kind}`
      : `env_${r.key}_${r.status}`;
  const line = t(key as ServerI18nKey, r.data);
  const prefix = r.status === "pass" ? "  ✓" : r.status === "warn" ? "  ⚠" : "  ✗";
  const first = `${prefix} ${r.key} — ${line}  (${r.status})`;

  const extras: string[] = [];
  if (typeof r.data.missing_bins === "string" && r.data.missing_bins)
    extras.push(`    missing: ${r.data.missing_bins}`);
  if (typeof r.data.stale_bins === "string" && r.data.stale_bins)
    extras.push(`    stale: ${r.data.stale_bins}`);
  if (typeof r.data.fresh_bins === "string" && r.data.fresh_bins)
    extras.push(`    fresh: ${r.data.fresh_bins}`);

  const desc = (envDescribeMap[r.key as EnvName]?.[locale === "zh-cn" ? "zh" : "en"] || "").trim();
  const lines = [first, ...extras];
  return desc ? `${lines.join("\n")}\n  ${desc}` : lines.join("\n");
}
