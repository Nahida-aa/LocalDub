import { ClientOnly } from "@tanstack/solid-router";
import { Code, Globe, Keyboard, Monitor, Palette, Settings, Server } from "lucide-solid";
import { type Component } from "solid-js";
import { Tabs, TabsContent, TabsIndicator, TabsList, TabsTrigger } from "@repo/ui-solid/base/tabs";
import { Modal } from "@repo/ui-solid/custom/modal/modal";
import type { JSX } from "solid-js";
import { openModal } from "@repo/ui-solid/custom/modal/renderer";
import { GeneralSettings } from "./general";
import { ServerManager } from "./ServerManager";
import { DeviceInfo } from "./DeviceInfo";
import { ProxyConfig } from "./ProxyConfig";
// import { useClientApi } from "../api/context";
import { i18n } from "@repo/shared/i18n/utils";
import { FileEditor } from "../FileContent/FileEditor";

export const SettingsContent = () => {
  const baseItems = [
    {
      value: "general",
      label: i18n.general(),
      icon: Settings,
    },
    {
      value: "shortcuts",
      label: i18n.shortcuts(),
      icon: Keyboard,
    },
    { value: "servers", label: "Servers", icon: Server as typeof Settings },
    { value: "device", label: "Device", icon: Monitor as typeof Settings },
    { value: "proxy", label: "Proxy", icon: Globe as typeof Settings },
    { value: "config", label: "Config", icon: Code as typeof Settings },
  ];
  return (
    <ClientOnly>
      <Tabs defaultValue="general" orientation="vertical" class="gap-5 h-full">
        <TabsList class="mb-4" variant="side">
          {baseItems.map((item) => (
            <TabsTrigger value={item.value} class="gap-2">
              <item.icon size={16} /> {item.label}
            </TabsTrigger>
          ))}
        </TabsList>
        <TabsContent value="general">
          <GeneralSettings />
        </TabsContent>
        <TabsContent value="shortcuts">
          <h2>{i18n.shortcuts()}</h2>
        </TabsContent>
        <TabsContent value="servers">
          <ServerManager />
        </TabsContent>
        <TabsContent value="device">
          <DeviceInfo />
        </TabsContent>
        <TabsContent value="proxy">
          <ProxyConfig />
        </TabsContent>
        <TabsContent value="config">
          <FileEditor path="packages/cli/input.json" label="input.json" />
        </TabsContent>
      </Tabs>
    </ClientOnly>
  );
};

export const openSettings = () => openModal(SettingsContent, { size: "5xl", class: "p-4 " });
