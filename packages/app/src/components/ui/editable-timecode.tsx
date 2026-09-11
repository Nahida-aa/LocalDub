import { cn } from "@repo/shared/lib/utils";
import { createEffect, createSignal } from "solid-js";
import type { FrameRate } from "@repo/util/timecode";
import { msToTimecode, timecodeToMs } from "@repo/util/timecode";

export type EditableTimecodeFormat = "timecode" | "ms" | "full";

interface EditableTimecodeProps {
  time: number;
  duration: number;
  fps: FrameRate;
  format?: EditableTimecodeFormat;
  onTimeChange?: (ms: number) => void;
  class?: string;
  disabled?: boolean;
}

const displayValue = (time: number, fps: FrameRate, format: EditableTimecodeFormat): string => {
  switch (format) {
    case "timecode":
      return msToTimecode(time, fps);
    case "ms":
      return String(Math.round(time % 1000)).padStart(3, "0");
    case "full":
      return String(Math.round(time));
  }
};

const parseValue = (
  input: string,
  currentTime: number,
  duration: number,
  fps: FrameRate,
  format: EditableTimecodeFormat,
): number | null => {
  switch (format) {
    case "timecode": {
      const parsed = timecodeToMs(input, fps);
      if (parsed == null) return null;
      return Math.max(0, Math.min(parsed, duration));
    }
    case "ms": {
      const v = parseInt(input, 10);
      if (isNaN(v) || v < 0 || v > 999) return null;
      const base = Math.floor(Math.max(0, currentTime) / 1000) * 1000;
      return Math.max(0, Math.min(base + v, duration));
    }
    case "full": {
      const v = parseInt(input, 10);
      if (isNaN(v) || v < 0) return null;
      return Math.max(0, Math.min(v, duration));
    }
  }
};

export function EditableTimecode(props: EditableTimecodeProps) {
  const format = () => props.format ?? "timecode";
  const [isEditing, setIsEditing] = createSignal(false);
  const [inputValue, setInputValue] = createSignal("");
  const [hasError, setHasError] = createSignal(false);
  let inputRef: HTMLInputElement | undefined;
  let enterPressed = false;

  const displayText = () => displayValue(props.time, props.fps, format());

  const startEditing = () => {
    if (props.disabled) return;
    setIsEditing(true);
    setInputValue(displayText());
    setHasError(false);
    enterPressed = false;
  };

  const cancelEditing = () => {
    setIsEditing(false);
    setInputValue("");
    setHasError(false);
    enterPressed = false;
  };

  const applyEdit = () => {
    const parsed = parseValue(inputValue(), props.time, props.duration, props.fps, format());
    if (parsed == null) {
      setHasError(true);
      return;
    }
    props.onTimeChange?.(parsed);
    setIsEditing(false);
    setInputValue("");
    setHasError(false);
    enterPressed = false;
  };

  createEffect(() => {
    if (isEditing() && inputRef) {
      inputRef.focus();
      inputRef.select();
    }
  });

  return (
    <>
      {isEditing() ? (
        <input
          ref={inputRef!}
          type="text"
          value={inputValue()}
          onChange={(e) => {
            setInputValue(e.currentTarget.value);
            setHasError(false);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              enterPressed = true;
              applyEdit();
            } else if (e.key === "Escape") {
              e.preventDefault();
              cancelEditing();
            }
          }}
          onBlur={() => {
            if (!enterPressed && isEditing()) applyEdit();
          }}
          class={cn(
            "-mx-1 border border-transparent bg-transparent px-1 font-mono text-xs outline-none",
            "focus:bg-background focus:border-primary focus:rounded",
            "text-primary tabular-nums",
            hasError() && "text-destructive focus:border-destructive",
            props.class,
          )}
          style={{ width: `${displayText().length + 2}ch` }}
          placeholder={displayText()}
        />
      ) : (
        <button
          type="button"
          onClick={startEditing}
          onKeyDown={(e) => {
            if (props.disabled) return;
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              startEditing();
            }
          }}
          disabled={props.disabled}
          class={cn(
            "text-primary cursor-pointer font-mono text-xs tabular-nums",
            "hover:bg-muted/50 -mx-1 px-1 hover:rounded",
            props.disabled && "cursor-default hover:bg-transparent",
            props.class,
          )}
          title={props.disabled ? undefined : "Click to edit time"}
        >
          {displayText()}
        </button>
      )}
    </>
  );
}
