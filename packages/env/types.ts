export type CheckStatus = "pass" | "fail" | "warn" | "skip" | "info";
export type EnvCategory = "core" | "optional" | "recommended" | "windows-only";

export type CheckResult = {
  key: string;
  status: CheckStatus;
  data: Record<string, string | number | boolean>;
  required: boolean;
};
