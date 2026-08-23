export interface TaskStage {
  completed_at?: string | null | undefined;
  error_message?: string | null | undefined;
  label: string;
  last_message?: string | null | undefined;
  name: string;
  progress?: number | null | undefined;
  started_at?: string | null | undefined;
  status?: StageStatus;
}
const stage_status_list = ["pending", "running", "success", "failed"] as const;

export type StageStatus = (typeof stage_status_list)[number];
