import { IndexPage } from "#/components/pages/home/IndexPage.tsx";
import { createFileRoute, redirect } from "@tanstack/solid-router";

export const Route = createFileRoute("/")({
  beforeLoad: () => {
    if (typeof window !== "undefined") {
      const last = localStorage.getItem("localdub_last_video");
      if (last) {
        try {
          const { groupId, workflowId } = JSON.parse(last);
          throw redirect({ to: "/group/$id/$workflowId", params: { id: groupId, workflowId } });
        } catch {
          localStorage.removeItem("localdub_last_video");
        }
      }
    }
  },
  component: IndexPage,
});
