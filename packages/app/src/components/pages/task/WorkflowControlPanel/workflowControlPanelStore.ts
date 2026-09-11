import { StageName } from "@repo/sdk/index";
import { createStore, useSelector } from "@tanstack/solid-store";

export type StageTab = StageName | "root";
interface WorkflowControlPanelStore {
  viewingTab: StageTab;
  runningStage: StageTab;
  resumeFrom?: StageName | null;
}
export const workflowControlPanelStore = createStore<WorkflowControlPanelStore>({
  viewingTab: "root",
  runningStage: "root",
  resumeFrom: null,
});

export const useViewingTab = () =>
  useSelector(workflowControlPanelStore, (state) => state.viewingTab);
export const setViewingTab = (tab?: StageTab | null) =>
  workflowControlPanelStore.setState((state) => ({
    ...state,
    viewingTab: tab ?? "root",
  }));

export const useRunningStage = () =>
  useSelector(workflowControlPanelStore, (state) => state.runningStage);
export const setRunningStage = (stage?: StageTab | null) =>
  workflowControlPanelStore.setState((state) => ({
    ...state,
    runningStage: stage ?? "root",
  }));

export const use_resumeFrom = () =>
  useSelector(workflowControlPanelStore, (state) => state.resumeFrom);
export const set_resumeFrom = (stage?: StageName | null) =>
  workflowControlPanelStore.setState((state) => ({
    ...state,
    resumeFrom: stage,
  }));
