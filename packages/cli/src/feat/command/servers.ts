import { InputArgs } from "@repo/core/input/input";
import { findServer, findServerViaMdnsAll } from "../../../../core/servers/discovery";
import {
  startVoxCPMTorchGradioServer,
  stopVoxCPMTorchGradioServer,
  voxcpmTorchGradioStatus,
} from "@repo/core/ml/voxcpm/runtime/voxcpm_torch_gradio";

export const cmdServers = async (input: InputArgs) => {
  const action = input.servers?.action ?? "status";
  const name = input.servers?.name;

  if (action === "stop") {
    if (!name || name === "voxcpm_torch_gradio") {
      const { port } = await findServer("voxcpm_torch_gradio");
      await stopVoxCPMTorchGradioServer({ port });
      console.log(`[Servers] VoxCPM server (port ${port}) stopped`);
    }
  } else if (action === "start") {
    if (!name || name === "voxcpm_torch_gradio") {
      const { port } = await findServer("voxcpm_torch_gradio");
      const { url } = await startVoxCPMTorchGradioServer({ port });
      console.log(`[Servers] VoxCPM PyTorch Gradio server ready at ${url}`);
    }
  } else if (action === "status") {
    const result: Record<string, unknown> = {};
    if (!name || name === "voxcpm_torch_gradio") {
      const { port } = await findServer("voxcpm_torch_gradio");
      result.voxcpm_torch_gradio = await voxcpmTorchGradioStatus({ port });
    }
    console.log(JSON.stringify(result, null, 2));
  } else if (action === "discovery") {
    const res = await findServerViaMdnsAll(name ?? "voxcpm_torch_gradio");
    console.log(res);
  } else {
    console.error(`[Servers] Unknown action: ${action}`);
  }
};
