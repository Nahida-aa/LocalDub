import type { MonacoTheme } from "monaco-themes";

const STORAGE_KEY = "editor-theme";

// monaco-themes 0.4.8 的 exports 不含 ./themes/*，无法直接 import 子路径，
// 因此把 8 个主题 JSON 复制到 src/assets/monaco-themes/ 静态导入。
const THEME_MODULES: Record<string, () => Promise<{ default: any }>> = {
  Dracula: () => import("../../../assets/monaco-themes/Dracula.json"),
  "GitHub Dark": () => import("../../../assets/monaco-themes/GitHub Dark.json"),
  "GitHub Light": () => import("../../../assets/monaco-themes/GitHub Light.json"),
  Monokai: () => import("../../../assets/monaco-themes/Monokai.json"),
  "Solarized-dark": () => import("../../../assets/monaco-themes/Solarized-dark.json"),
  "Solarized-light": () => import("../../../assets/monaco-themes/Solarized-light.json"),
  "Night Owl": () => import("../../../assets/monaco-themes/Night Owl.json"),
  Nord: () => import("../../../assets/monaco-themes/Nord.json"),
};

export const EDITOR_THEMES = [
  { value: "Dracula", label: "Dracula" },
  { value: "GitHub Dark", label: "GitHub Dark" },
  { value: "GitHub Light", label: "GitHub Light" },
  { value: "Monokai", label: "Monokai" },
  { value: "Solarized-dark", label: "Solarized Dark" },
  { value: "Solarized-light", label: "Solarized Light" },
  { value: "Night Owl", label: "Night Owl" },
  { value: "Nord", label: "Nord" },
] as const;

export type EditorThemeName = (typeof EDITOR_THEMES)[number]["value"];

export function getEditorTheme(): EditorThemeName {
  if (typeof localStorage === "undefined") return "Dracula";
  return (localStorage.getItem(STORAGE_KEY) as EditorThemeName) || "Dracula";
}

export function setEditorTheme(theme: string): void {
  if (typeof localStorage === "undefined") return;
  localStorage.setItem(STORAGE_KEY, theme);
}

export async function loadEditorTheme(monaco: typeof import("monaco-editor"), themeName: string) {
  try {
    const loader = THEME_MODULES[themeName];
    if (!loader) throw new Error(`unknown theme: ${themeName}`);
    const themeData = (await loader()).default;
    monaco.editor.defineTheme(themeName, themeData);
    monaco.editor.setTheme(themeName);
  } catch {
    monaco.editor.setTheme("vs-dark");
  }
}
