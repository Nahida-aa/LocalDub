export interface FrameRate {
  numerator: number; // 分子
  denominator: number; // 分母
}

/**
 * 将 HH:MM:SS:FF 格式的时间码字符串解析为毫秒。
 *
 * - 帧范围验证：FF < ceil(numerator / denominator)
 * - 内部使用 Math.round 将帧偏移转换为毫秒
 *
 * @param tc - 时间码字符串 (e.g. "01:02:03:15")
 * @param fps - 有理数帧率 { numerator, denominator }
 * @returns 毫秒值，解析失败返回 null
 */
export function timecodeToMs(tc: string, fps: FrameRate): number | null {
  const parts = tc.split(":");
  if (parts.length !== 4) return null;
  const [h, m, s, f] = parts.map(Number);
  if ([h, m, s, f].some(Number.isNaN)) return null;
  if (m < 0 || m > 59 || s < 0 || s > 59) return null;
  const fpsFloat = fps.numerator / fps.denominator;
  const frameUpperBound = Math.ceil(fpsFloat);
  if (f < 0 || f >= frameUpperBound) return null;
  const totalMs = h * 3600000 + m * 60000 + s * 1000;
  return totalMs + Math.round((f / fpsFloat) * 1000);
}

/**
 * 将毫秒值格式化为 HH:MM:SS:FF 时间码。
 *
 * - 帧号通过 floor 计算，对应 OpenCutClassic 的 integer division: second_ticks / ticks_per_frame
 *
 * @param ms - 毫秒值
 * @param fps - 有理数帧率 { numerator, denominator }
 * @returns 格式化的时间码字符串 (e.g. "01:02:03:15")
 */
export function msToTimecode(ms: number, fps: FrameRate): string {
  const total = Math.max(0, ms);
  const h = Math.floor(total / 3600000);
  const m = Math.floor((total % 3600000) / 60000);
  const s = Math.floor((total % 60000) / 1000);
  const remainingMs = total % 1000;
  const fpsFloat = fps.numerator / fps.denominator;
  // floor() 对应 OpenCutClassic 的 integer division: second_ticks / ticks_per_frame
  const frame = Math.floor((remainingMs / 1000) * fpsFloat);
  const frameUpperBound = Math.ceil(fpsFloat);
  const frameClamped = Math.min(frame, frameUpperBound - 1);
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}:${String(frameClamped).padStart(2, "0")}`;
}

/**
 * 将毫秒值格式化为 HH:MM:SS:FF(mmm, full_ms) 格式，同时显示帧时间码和毫秒信息。
 *
 * 格式：HH:MM:SS:FF(mmm, full_ms)
 * - HH:MM:SS:FF — 帧时间码（同 msToTimecode）
 * - mmm — 当前秒内的毫秒偏移 (0–999)
 * - full_ms — 总毫秒值
 *
 * @param ms - 毫秒值
 * @param fps - 有理数帧率 { numerator, denominator }
 * @returns 格式化的字符串 (e.g. "00:00:01:15(500, 1500)")
 */
export function msToTimecodeFull(ms: number, fps: FrameRate): string {
  const total = Math.max(0, ms);
  const remainingMs = total % 1000;
  const tc = msToTimecode(total, fps);
  return `${tc}(${Math.round(remainingMs)},${Math.round(total)})`;
}

/**
 * 将 HH:MM:SS:FF(mmm, full_ms) 完整格式解析为毫秒。
 *
 * 同时解析帧时间码和括号中的毫秒值，二者必须一致（误差 ≤ 1ms）。
 *
 * @param tc - 完整格式字符串 (e.g. "00:00:01:15(500, 1500)")
 * @param fps - 有理数帧率 { numerator, denominator }
 * @returns 毫秒值，解析或校验不通过返回 null
 */
export function timecodeFullToMs(tc: string, fps: FrameRate): number | null {
  const m = tc.match(/^(\d{2}:\d{2}:\d{2}:\d{2})\((\d+),(\d+)\)$/);
  if (!m) return null;
  const tcParsed = timecodeToMs(m[1], fps);
  if (tcParsed == null) return null;
  const bracketMs1 = parseInt(m[2], 10);
  const bracketMs2 = parseInt(m[3], 10);
  if (isNaN(bracketMs1) || isNaN(bracketMs2)) return null;
  if (Math.abs(tcParsed - bracketMs2) > 1) return null;
  return bracketMs2;
}
