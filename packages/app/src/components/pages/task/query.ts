import { client } from "#/integrations/fnrpc/client.ts";
import { AsrOcrFile } from "@repo/subtitle-ocr/types";
import { TranslateResult } from "@repo/core/stages/05_translate/out";
import { SplitAudioResult, SplitAudioTimingResult } from "@repo/core/stages/06_split_audio/out";
import { TtsFile } from "@repo/core/stages/07_tts/out";
import { AsrResult } from "@repo/core/stages/asr/out";
import { TimingsFile } from "@repo/core/stages/mix_audio/types";
import { stages_to_map } from "@repo/core/stages/utils/filtering";
import { to } from "@repo/shared/lib/utils/try";
import { useQuery } from "@tanstack/solid-query";
import { useParams } from "@tanstack/solid-router";

export const use_task_ctx = () => {
  const p = useParams({ from: "/group/$id/$taskId" });
  const taskDir = `workfolder/${p().id}/${p().taskId}`;
  return useQuery(() => client.get_task_ctx.queryOptions(taskDir));
};

interface TrackProps {
  enabled?: () => boolean;
}

export const use_asr_data = (props: TrackProps) => {
  const p = useParams({ from: "/group/$id/$taskId" });
  const asrQuery = useQuery(() =>
    client.read_app_file_text.queryOptions(`workfolder/${p().id}/${p().taskId}/asr/asr.json`, {
      enabled: props.enabled?.(),
    }),
  );
  const asrSegments = () => {
    if (!asrQuery.data) return [];
    const [data, err] = to<AsrResult>(() => JSON.parse(asrQuery.data));
    if (err) return [];
    return (data.result?.segments || [])
      .map((s, i: number) => ({
        index: i,
        text: (s.text || "").trim(),
        startMs: s.start_ms,
        endMs: s.end_ms,
      }))
      .filter((s: { text: string }) => s.text);
  };
  return asrSegments;
};

export const use_split_audio_timings_data = (props: TrackProps) => {
  const p = useParams({ from: "/group/$id/$taskId" });
  const split_audio_timings_q = useQuery(() =>
    client.read_app_file_text.queryOptions(
      `workfolder/${p().id}/${p().taskId}/split_audio/timings.json`,
      {
        enabled: props.enabled?.(),
      },
    ),
  );
  const split_audio_timings = () => {
    if (!split_audio_timings_q.data) return [];
    const [data, err] = to<SplitAudioTimingResult>(() => JSON.parse(split_audio_timings_q.data));
    if (err) return [];
    return (data.segments || []).map((item, i: number) => ({
      index: i,
      text: item.dst,
      startMs: item.start_ms,
      endMs: item.end_ms,
      raw: item,
    }));
  };
  return split_audio_timings;
};

export const use_tts_data = (props: TrackProps) => {
  const p = useParams({ from: "/group/$id/$taskId" });
  const tts_q = useQuery(() =>
    client.read_app_file_text.queryOptions(`workfolder/${p().id}/${p().taskId}/tts/tts.json`, {
      enabled: props.enabled?.(),
    }),
  );
  const tts = () => {
    if (!tts_q.data) return [];
    const [data, err] = to<TtsFile>(() => JSON.parse(tts_q.data));
    if (err) return [];
    return (data.segments || []).map((item, i: number) => ({
      index: i,
      text: item.text,
      startMs: item.start_ms,
      endMs: item.end_ms,
      raw: item,
    }));
  };
  return tts;
};

export const use_translate_data = (props: TrackProps) => {
  const p = useParams({ from: "/group/$id/$taskId" });
  const taskCtxQ = use_task_ctx();
  const transLang = () => taskCtxQ.data?.target_language;
  const translation_q = useQuery(() =>
    client.read_app_file_text.queryOptions(
      `workfolder/${p().id}/${p().taskId}/translate/translation.${transLang()}.json`,
      {
        enabled: props.enabled?.(),
      },
    ),
  );
  const translation = () => {
    if (!translation_q.data) return [];
    const [data, err] = to<TranslateResult>(() => JSON.parse(translation_q.data));
    if (err) return [];
    return (data.segments || []).map((item, i: number) => ({
      index: i,
      text: item.dst,
      startMs: item.start_ms,
      endMs: item.end_ms,
      raw: item,
    }));
  };
  return translation;
};

export const use_mix_audio_timings_data = (props: TrackProps) => {
  const p = useParams({ from: "/group/$id/$taskId" });
  const mix_audio_q = useQuery(() =>
    client.read_app_file_text.queryOptions(
      `workfolder/${p().id}/${p().taskId}/mix_audio/timings.json`,
      {
        enabled: props.enabled?.(),
      },
    ),
  );
  const mix_audio = () => {
    if (!mix_audio_q.data) return [];
    const [data, err] = to<TimingsFile>(() => JSON.parse(mix_audio_q.data));
    if (err) return [];
    return (data.segments || []).map((item, i: number) => ({
      index: i,
      text: item.dst,
      startMs: item.start_ms,
      endMs: item.end_ms,
      raw: item,
    }));
  };
  return mix_audio;
};

export const use_split_audio_data = (props: TrackProps) => {
  const p = useParams({ from: "/group/$id/$taskId" });
  const split_audio_q = useQuery(() =>
    client.read_app_file_text.queryOptions(
      `workfolder/${p().id}/${p().taskId}/split_audio/split_audio.json`,
      {
        enabled: props.enabled?.(),
      },
    ),
  );
  const split_audio = () => {
    if (!split_audio_q.data) return [];
    const [data, err] = to<SplitAudioResult>(() => JSON.parse(split_audio_q.data));
    if (err) return [];
    return (data.segments || []).map((item, i: number) => ({
      index: i,
      text: item.dst,
      startMs: item.start_ms,
      endMs: item.end_ms,
      raw: item,
    }));
  };
  return split_audio;
};

export const use_asr_ocr_fix_llm_data = (props: TrackProps) => {
  const p = useParams({ from: "/group/$id/$taskId" });
  const asr_ocr_fix_llm_q = useQuery(() =>
    client.read_app_file_text.queryOptions(
      `workfolder/${p().id}/${p().taskId}/asr_ocr_fix/asr_ocr_fused_llm_fix.json`,
      {
        enabled: props.enabled?.(),
      },
    ),
  );
  const asr_ocr_fix_llm = () => {
    if (!asr_ocr_fix_llm_q.data) return [];
    const [data, err] = to<AsrOcrFile>(() => JSON.parse(asr_ocr_fix_llm_q.data));
    if (err) return [];
    return (data.result.segments || []).map((item, i: number) => ({
      index: i,
      text: item.text,
      startMs: item.start_ms,
      endMs: item.end_ms,
      raw: item,
    }));
  };
  return asr_ocr_fix_llm;
};
