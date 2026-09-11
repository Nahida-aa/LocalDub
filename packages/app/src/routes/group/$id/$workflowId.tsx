import { createFileRoute, useParams } from "@tanstack/solid-router";
import { onMount } from "solid-js";
import { VideoDetailPage } from "../../../components/pages/task/VideoDetailPage";

export const Route = createFileRoute("/group/$id/$workflowId")({
  component: RouteComponent,
});

function RouteComponent() {
  const p = useParams({ from: "/group/$id/$workflowId" });
  onMount(() => {
    localStorage.setItem(
      "localdub_last_video",
      JSON.stringify({
        groupId: p().id,
        workflowId: p().workflowId,
      }),
    );
  });
  return <VideoDetailPage groupId={p().id} workflowId={p().workflowId} />;
}
