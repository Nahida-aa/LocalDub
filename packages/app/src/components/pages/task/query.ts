import { client } from "#/integrations/fnrpc/client.ts";
import { AsrOcrFile } from "@repo/core/ml/subtitle_ocr/types";
import { TranslateFile } from "@repo/core/stages/05_translate/type";
import { SplitAudioFile, SplitAudioTimingFile } from "@repo/core/stages/06_split_audio/types";
import { TtsFile } from "@repo/core/stages/07_tts/types";
import { AsrResult } from "@repo/core/stages/asr/types";
import { TimingsFile } from "@repo/core/stages/merge_audio/types";
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
        startMs: s.start,
        endMs: s.end,
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
    const [data, err] = to<SplitAudioTimingFile>(() => JSON.parse(split_audio_timings_q.data));
    if (err) return [];
    return (data.translation || []).map((item, i: number) => ({
      index: i,
      text: item.dst,
      startMs: item.start,
      endMs: item.end,
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
      startMs: item.start,
      endMs: item.end,
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
    const [data, err] = to<TranslateFile>(() => JSON.parse(translation_q.data));
    if (err) return [];
    return (data.translation || []).map((item, i: number) => ({
      index: i,
      text: item.dst,
      startMs: item.start,
      endMs: item.end,
      raw: item,
    }));
  };
  return translation;
};

export const use_merge_audio_timings_data = (props: TrackProps) => {
  const p = useParams({ from: "/group/$id/$taskId" });
  const merge_audio_q = useQuery(() =>
    client.read_app_file_text.queryOptions(
      `workfolder/${p().id}/${p().taskId}/merge_audio/timings.json`,
      {
        enabled: props.enabled?.(),
      },
    ),
  );
  const merge_audio = () => {
    if (!merge_audio_q.data) return [];
    const [data, err] = to<TimingsFile>(() => JSON.parse(merge_audio_q.data));
    if (err) return [];
    return (data.translation || []).map((item, i: number) => ({
      index: i,
      text: item.dst,
      startMs: item.start,
      endMs: item.end,
      raw: item,
    }));
  };
  return merge_audio;
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
    const [data, err] = to<SplitAudioFile>(() => JSON.parse(split_audio_q.data));
    if (err) return [];
    return (data.translation || []).map((item, i: number) => ({
      index: i,
      text: item.dst,
      startMs: item.start,
      endMs: item.end,
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
      startMs: item.start,
      endMs: item.end,
      raw: item,
    }));
  };
  return asr_ocr_fix_llm;
};
