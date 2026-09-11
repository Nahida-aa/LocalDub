import { createFileRoute, useParams } from "@tanstack/solid-router";
import { onMount } from "solid-js";
import { VideoDetailPage } from "../../../components/pages/task/VideoDetailPage";

export const Route = createFileRoute("/group/$id/$videoId")({
  component: RouteComponent,
});

function RouteComponent() {
  const p = useParams({ from: "/group/$id/$videoId" });
  onMount(() => {
    localStorage.setItem(
      "localdub_last_video",
      JSON.stringify({
        groupId: p().id,
        videoId: p().videoId,
      }),
    );
  });
  return <VideoDetailPage groupId={p().id} videoId={p().videoId} />;
}
