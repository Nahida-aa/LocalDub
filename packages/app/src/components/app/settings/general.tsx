import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@repo/ui-solid/base/select";
import { CardX } from "@repo/ui-solid/custom/card";
import { createSignal } from "solid-js";
import { getLocale, m, setLocale, tLocaleName, type LocaleNameKey } from "@repo/shared/i18n/utils";
import { useTheme } from "@repo/ui-solid/theme";
import { THEMES } from "@repo/ui-solid/theme/defs";
import { getAutoSaveMode, setAutoSaveMode } from "./editorPrefs";
import { getLogLevel, setLogLevel, type LogLevel } from "#/lib/debugLog.ts";
import type { AutoSaveMode } from "./editorPrefs";
const languages = [
  { value: "en", label: tLocaleName("en") },
  { value: "zh-cn", label: tLocaleName("zh-cn") },
];
type LanguageOption = (typeof languages)[number];
const autoSaveOptions: { value: string; label: string }[] = [
  { value: "afterDelay", label: "After Delay" },
  { value: "off", label: "Off" },
];
type AutoSaveOption = { value: string; label: string };
const logLevelOptions: { value: LogLevel; label: string }[] = [
  { value: "off", label: "Off" },
  { value: "trace", label: "Trace" },
  { value: "debug", label: "Debug" },
  { value: "info", label: "Info" },
  { value: "warn", label: "Warn" },
];
export const GeneralSettings = () => {
  const [autoSaveMode, setAutoSaveModeState] = createSignal(getAutoSaveMode());
  const [logLevel, setLogLevelState] = createSignal(getLogLevel());

  return (
    <div>
      <h2>{m.general()}</h2>
      <CardX
        title={m.language()}
        description={m.settings_language_description()}
        size="sm"
        Footer={
          <Select
            value={{
              value: getLocale(),
              label: tLocaleName(getLocale()),
            }}
            optionValue="value"
            optionTextValue="label"
            onChange={(v) => setLocale(v?.value ?? "en")}
            options={languages}

            itemComponent={(props) => (
              <SelectItem item={props.item}>{props.item.rawValue.label}</SelectItem>
            )}
          >
            <SelectTrigger class="w-45">
              <SelectValue<LanguageOption>>{(state) => state.selectedOption().label}</SelectValue>
            </SelectTrigger>
            <SelectContent />
          </Select>
        }
      />
      <CardX
        title="Files: Auto Save"
        description={m.settings_auto_save()}
        size="sm"
        Footer={
          <Select
            value={{
              value: autoSaveMode(),
              label: autoSaveMode() === "afterDelay" ? "After Delay" : "Off",
            }}
            optionValue="value"
            optionTextValue="label"
            onChange={(v) => {
              const mode = (v?.value ?? "afterDelay") as AutoSaveMode;
              setAutoSaveModeState(mode);
              setAutoSaveMode(mode);
            }}
            options={autoSaveOptions}
            itemComponent={(props) => (
              <SelectItem item={props.item}>{props.item.rawValue.label}</SelectItem>
            )}
          >
            <SelectTrigger class="w-30">
              <SelectValue<AutoSaveOption>>{(state) => state.selectedOption().label}</SelectValue>
            </SelectTrigger>
            <SelectContent />
          </Select>
        }
      />
      <CardX
        title="Debug Logs"
        description="控制前端调试日志输出级别 (TRACE-ctx / Timeline 等)，Trace 显示全部"
        size="sm"
        Footer={
          <Select
            value={{ value: logLevel(), label: logLevel().toUpperCase() }}
            optionValue="value"
            optionTextValue="label"
            onChange={(v) => {
              const level = (v?.value ?? "warn") as LogLevel;
              setLogLevelState(level);
              setLogLevel(level);
            }}
            options={logLevelOptions}
            itemComponent={(props) => (
              <SelectItem item={props.item}>{props.item.rawValue.label}</SelectItem>
            )}
          >
            <SelectTrigger class="w-30">
              <SelectValue<AutoSaveOption>>{(state) => state.selectedOption().label}</SelectValue>
            </SelectTrigger>
            <SelectContent />
          </Select>
        }
      />
      <AppearanceSettings />
    </div>
  );
};

const AppearanceSettings = () => {
  const { colorScheme, setColorScheme, themeName, setThemeName, currentThemeDef } = useTheme();
  const themeOptions = THEMES.map((t) => ({ value: t.value, label: t.label }));
  const schemeOptions = [
    { value: "system", label: m.system() },
    { value: "light", label: m.light() },
    { value: "dark", label: m.dark() },
  ];
  type SchemeOption = { value: string; label: string };

  return (
    <>
      <h3>{m.appearance()}</h3>
      <CardX
        title={m.color_scheme()}
        description={m.settings_color_scheme()}
        size="sm"
        Footer={
          <Select
            value={{ value: colorScheme(), label: colorScheme() }}
            optionValue="value"
            optionTextValue="label"
            onChange={(v) => {
              if (v?.value) setColorScheme(v.value as any);
            }}
            options={schemeOptions}
            itemComponent={(props) => (
              <SelectItem item={props.item}>{props.item.rawValue.label}</SelectItem>
            )}
          >
            <SelectTrigger class="w-40">
              <SelectValue<SchemeOption>>{(state) => state.selectedOption().label}</SelectValue>
            </SelectTrigger>
            <SelectContent />
          </Select>
        }
      />
      <CardX
        title="Theme"
        description="Color palette (affects editor too)"
        size="sm"
        Footer={
          <Select
            value={{ value: themeName(), label: currentThemeDef()?.label ?? themeName() }}
            optionValue="value"
            optionTextValue="label"
            onChange={(v) => {
              if (v?.value) setThemeName(v.value);
            }}
            options={themeOptions}
            itemComponent={(props) => (
              <SelectItem item={props.item}>{props.item.rawValue.label}</SelectItem>
            )}
          >
            <SelectTrigger class="w-50">
              <SelectValue<SchemeOption>>{(state) => state.selectedOption().label}</SelectValue>
            </SelectTrigger>
            <SelectContent />
          </Select>
        }
      />
    </>
  );
};
