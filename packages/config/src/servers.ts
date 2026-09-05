export const serverTypeList = ["main", "voxcpm_torch_gradio"] as const;
export type ServerType = (typeof serverTypeList)[number];

export const SERVICE_MAP: Record<ServerType, string> = {
  main: "_ld-main._tcp.local",
  voxcpm_torch_gradio: "_ld-voxcpm-py._tcp.local",
};
