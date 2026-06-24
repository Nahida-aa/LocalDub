# Windows PATH 大小写坑 — spawnSync DLL 找不到 exit=53

## 现象

调用 C++ 二进制（`ocr_pipeline.exe`、`subtitle_ocr_ort_cpp.exe`、`whisper.exe` 等）时：

- 退出码 `exit=53`
- `stderr` 和 `stdout` 均为空
- 二进制文件存在，DLL 文件也存在于 build 目录
- `msys2Bin exists=true`, `ortLib exists=true`, `buildDir exists=true`
- 手动在 PowerShell 里设置 PATH 后直接运行二进制正常

## 根因

**Windows 环境变量 PATH 的键名大小写问题**。

Windows 上 `process.env` 中 PATH 的实际键名是 `Path`（首字母大写），而不是 `PATH`（全大写）。

当代码这样写时：

```typescript
const LIB_PATH_KEY = process.platform === 'win32' ? 'PATH' : 'LD_LIBRARY_PATH';

const env = {
    ...process.env,           // 展开后得到 "Path": "C:\Windows\System32;..."
    [LIB_PATH_KEY]: libPath,  // 设置 "PATH": "C:\msys64\...;..."
};
```

结果 `env` 对象中同时存在 `Path`（原始系统 PATH，不含 DLL 目录）和 `PATH`（我们设置的，含 DLL 目录）两个键。

Windows 的 `spawnSync` 在启动子进程时，使用了原始的 `Path`（因为它是在展开时先出现的键，或者 Windows 内部按某种顺序查找），导致 DLL 目录不在 PATH 中，进程无法启动。

### 为什么退出码是 53？

退出码 53 = Windows 系统错误码 `ERROR_BAD_NETPATH`（"The network path was not found."）。

这是因为 `spawnSync` 创建进程失败时，将 Windows `CreateProcess` 的系统错误码直接作为 `status` 返回。**它不是程序的退出码，而是进程创建失败的错误码。** 这也是为什么 stdout/stderr 都是空的——进程根本没启动起来。

### 触发条件

当 Node.js/bun 进程的原始环境中 PATH 不包含所需 DLL 目录时（例如通过桌面快捷方式启动、或从不含这些路径的 shell 启动），此 bug 才会暴露。如果当前 shell 已经在 PATH 里有 DLL 目录（比如开发时手动设置过），就不会触发。

## 修复

先检测 `process.env` 中 PATH 的实际键名，再用该键名设置：

```typescript
function getLibPathKey(): string {
    if (process.platform !== 'win32') return 'LD_LIBRARY_PATH';
    const existing = Object.keys(process.env).find(k => k.toLowerCase() === 'path');
    return existing || 'PATH';
}
const LIB_PATH_KEY = getLibPathKey();
```

这样展开 `...process.env` 后再设置 `[LIB_PATH_KEY]` 时，会覆盖原来的同名键，而不是新增一个不同大小写的键。

## 受影响的文件（已修复）

| 文件 | 说明 |
|------|------|
| `packages/cli/src/ml/ocr/runtimes/ort-cpp.ts` | OCR OpenCV C++ 运行时 |
| `packages/cli/src/feat/stages/asr/asr.ts` | ASR whisper.cpp 运行时 |

## 排查 checklist

如果以后遇到类似的"二进制 exit=非0 且无任何输出"的问题：

1. **exit=53 且 stderr/stdout 为空** → 大概率是 PATH 大小写问题
2. 用 `Object.keys(process.env).find(k => k.toLowerCase() === 'path')` 检查实际键名
3. 确认设置环境变量时用的键名与展开 `process.env` 后的键名一致

## 参考

- [Node.js 文档：process.env](https://nodejs.org/api/process.html#processenv) — 在 Windows 上环境变量名不区分大小写
