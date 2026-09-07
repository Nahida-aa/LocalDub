import { ClientOnly } from "@tanstack/solid-router";
import {
  Code,
  Keyboard,
  Monitor,
  Palette,
  Settings,
  Server,
  SlidersHorizontal,
} from "lucide-solid";
import { type Component } from "solid-js";
import { Tabs, TabsContent, TabsIndicator, TabsList, TabsTrigger } from "@repo/ui-solid/base/tabs";
import { Modal } from "@repo/ui-solid/custom/modal/modal";
import type { JSX } from "solid-js";
import { openModal } from "@repo/ui-solid/custom/modal/renderer";
import { GeneralSettings } from "./general";
import { InputFormSettings } from "./inputForm";
import { ServerManager } from "./ServerManager";
import { DeviceInfo } from "./DeviceInfo";
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
    { value: "config", label: "input.jsonc", icon: Code as typeof Settings },
    {
      value: "input-form",
      label: "全局输入参数",
      icon: SlidersHorizontal as typeof Settings,
    },
  ];
  return (
    <ClientOnly>
      {/* h-full + grid-rows-[1fr] + min-h-0: 让内容列受模态框高度约束,
          各 TabsContent 再自行 overflow-y-auto (否则长内容顶出父容器) */}
      <Tabs
        defaultValue="general"
        orientation="vertical"
        class="gap-5 h-full min-h-0 grid-rows-[1fr]"
      >
        <TabsList class="mb-4" variant="side">
          {baseItems.map((item) => (
            <TabsTrigger value={item.value} class="gap-2">
              <item.icon size={16} /> {item.label}
            </TabsTrigger>
          ))}
        </TabsList>
        <TabsContent value="general" class="min-h-0 overflow-y-auto">
          <GeneralSettings />
        </TabsContent>
        <TabsContent value="shortcuts" class="min-h-0 overflow-y-auto">
          <h2>{i18n.shortcuts()}</h2>
        </TabsContent>
        <TabsContent value="servers" class="min-h-0 overflow-y-auto">
          <ServerManager />
        </TabsContent>
        <TabsContent value="device" class="min-h-0 overflow-y-auto">
          <DeviceInfo />
        </TabsContent>
        <TabsContent value="config" class="min-h-0">
          {/* 仓库根 input.jsonc: cli resolve_input_path 真正读取的配置文件 (优先于 input.json)。
              Monaco 自带滚动, 外层不再 overflow */}
          <FileEditor path="input.jsonc" label="input.jsonc" />
        </TabsContent>
        <TabsContent value="input-form" class="min-h-0 overflow-y-auto">
          <InputFormSettings />
        </TabsContent>
      </Tabs>
    </ClientOnly>
  );
};

export const openSettings = () => openModal(SettingsContent, { size: "5xl", class: "p-4 " });
