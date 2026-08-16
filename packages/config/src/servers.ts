export const serverTypeList = ["server", "voxcpm_torch_gradio", "demucs_torch_server"] as const;
export type ServerType = (typeof serverTypeList)[number];

export const SERVICE_MAP: Record<ServerType, string> = {
  server: "_ld-server._tcp.local",
  voxcpm_torch_gradio: "_ld-voxcpm-py._tcp.local",
  demucs_torch_server: "_ld-demucs-py._tcp.local",
};
