import { StepName } from "@repo/sdk/index";
import { createStore, useSelector } from "@tanstack/solid-store";

export type StepTab = StepName | "root";
interface WorkflowControlPanelStore {
  viewingTab: StepTab;
  runningStep: StepTab;
  resumeFrom?: StepName | null;
}
export const workflowControlPanelStore = createStore<WorkflowControlPanelStore>({
  viewingTab: "root",
  runningStep: "root",
  resumeFrom: null,
});

export const useViewingTab = () =>
  useSelector(workflowControlPanelStore, (state) => state.viewingTab);
export const setViewingTab = (tab?: StepTab | null) =>
  workflowControlPanelStore.setState((state) => ({
    ...state,
    viewingTab: tab ?? "root",
  }));

export const useRunningStep = () =>
  useSelector(workflowControlPanelStore, (state) => state.runningStep);
export const setRunningStep = (stage?: StepTab | null) =>
  workflowControlPanelStore.setState((state) => ({
    ...state,
    runningStep: stage ?? "root",
  }));

export const use_resumeFrom = () =>
  useSelector(workflowControlPanelStore, (state) => state.resumeFrom);
export const set_resumeFrom = (stage?: StepName | null) =>
  workflowControlPanelStore.setState((state) => ({
    ...state,
    resumeFrom: stage,
  }));
