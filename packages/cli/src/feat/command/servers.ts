import { InputArgs } from "@repo/core/input/input";
import { findServer } from "../../../../core/servers/discovery";

export const cmdServers = async (input: InputArgs) => {
  const action = input.servers?.action ?? "status";
  const name = input.servers?.name;

  if (action === "status") {
    const { port } = await findServer("main");
    const result: Record<string, unknown> = { main: { port } };
    console.log(JSON.stringify(result, null, 2));
  } else if (action === "discovery") {
    const res = await findServer(name ?? "main");
    console.log(res);
  } else {
    console.error(`[Servers] Unknown action: ${action}`);
  }
};
