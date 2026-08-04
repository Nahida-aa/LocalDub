# ort-cpp 二进制 GitHub Release 说明

## Release 位置

- Tag: `ort-cpp-v0.1.0`
- URL: https://github.com/Nahida-aa/LocalDub/releases/tag/ort-cpp-v0.1.0
- 资产: `subtitle_ocr_ort_cpp-linux-x64`（主程序）、`libonnxruntime.so.1`（ORT 1.24.4 运行时，需与主程序同目录）、`SHA256SUMS`

## 构建来源

Release 里的二进制是 **2026-07-10 从 commit `42dcd24` 编译**的（`packages/subtitle-ocr/ort-cpp/`），
而 tag 打在当前 `main` HEAD 上。该目录自 `42dcd24` 之后没有源码改动，因此 Release 二进制与从当前 HEAD 本地重新构建的二进制**功能等价**。

## 为什么 Release 二进制与本地新构建不是字节级相同

两者并非完全一致（sha256 不同），但差异全部来自构建产物元数据，与功能代码无关：

1. **GNU Build ID** — 链接器每次生成的唯一标识（`.note.gnu.build-id` 段）。
2. **工具链 codegen 噪声** — 系统 g++ 更新会导致 `.cold`（异常处理路径）函数排序/布局变化（约 64 字节）。

## 等价性验证（2026-08-02）

- `nm -C --defined-only` 符号集合完全一致
- 同帧 A/B 输出逐字段相同（文本、confidence、box 完全一致，仅 `*Ms` 计时有运行抖动）
- 删除死代码 `stb_image.h` 后重新构建，`bun test ocr.test.ts` 10/10 通过

## 注意事项

- **无需刷新 Release 资产**：功能一致，重新上传只有 build-id/时间戳差异，无实际价值。
- 运行时仍依赖系统 **OpenCV5**（`libopencv-core.so.500` 等，`apt install libopencv-dev`）+ 自备 rapidocr 模型目录与 `ppocr_keys.json`，详见 Release notes。
