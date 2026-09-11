// sync

import { createCollection, localStorageCollectionOptions } from "@tanstack/solid-db";
import { z } from "zod";

export const workflowGroupExpandCollection = createCollection(
  localStorageCollectionOptions({
    id: "workflow-group-expand",
    schema: z.object({
      id: z.string(),
    }),
    storageKey: "localdub_workflow_group_expand",
    getKey: (item) => item.id,
  }),
);
