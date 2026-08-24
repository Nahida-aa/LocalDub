import { StageName } from "@repo/core/tasks/args";
import { createStore, useSelector } from "@tanstack/solid-store";

export type StageTab = StageName | "root";
interface TaskControlPanelStore {
  viewingTab: StageTab;
  runningStage: StageTab;
  resumeFrom?: StageName | null;
}
export const taskControlPanelStore = createStore<TaskControlPanelStore>({
  viewingTab: "root",
  runningStage: "root",
  resumeFrom: null,
});

export const useViewingTab = () => useSelector(taskControlPanelStore, (state) => state.viewingTab);
export const setViewingTab = (tab?: StageTab | null) =>
  taskControlPanelStore.setState((state) => ({
    ...state,
    viewingTab: tab ?? "root",
  }));

export const useRunningStage = () =>
  useSelector(taskControlPanelStore, (state) => state.runningStage);
export const setRunningStage = (stage?: StageTab | null) =>
  taskControlPanelStore.setState((state) => ({
    ...state,
    runningStage: stage ?? "root",
  }));

export const use_resumeFrom = () => useSelector(taskControlPanelStore, (state) => state.resumeFrom);
export const set_resumeFrom = (stage?: StageName | null) =>
  taskControlPanelStore.setState((state) => ({
    ...state,
    resumeFrom: stage,
  }));
