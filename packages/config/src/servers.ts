export const serverTypeList = ["main"] as const;
export type ServerType = (typeof serverTypeList)[number];

export const SERVICE_MAP: Record<ServerType, string> = {
  main: "_ld-main._tcp.local",
};
