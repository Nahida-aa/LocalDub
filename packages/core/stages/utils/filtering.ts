import { StageName } from "../../tasks/args";
import { TaskStage } from "../../context/types";

export const stages_to_map = (
  stages: (TaskStage | undefined)[],
): Record<StageName, TaskStage | undefined> => {
  return stages.reduce(
    (acc, stage) => {
      if (stage === undefined) return acc;
      acc[stage.name as StageName] = stage;
      return acc;
    },
    {} as Record<StageName, TaskStage | undefined>,
  );
};
