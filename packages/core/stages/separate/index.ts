import { existsSync, mkdirSync, readdirSync } from "node:fs";
import { nowISO, separateDir, video_source_path } from "@repo/core/stages/utils/utils.ts";
import { TaskCtx, setStage } from "@repo/core/context/context.ts";
import { startLog } from "../utils/log.ts";
import { separateBurn } from "../../ml/demucs/cli/burn_cli.ts";
import { log } from "@repo/util/log";

export async function stageSeparate(ctx: TaskCtx) {
  startLog("separate", ctx.task.id);
  const taskId = ctx.task.id;
  const taskDir = ctx.task.task_dir;
  // subtitle 模式且未配置 always 时，跳过分离
  const pipeline = ctx.pipeline;
  const sepArgs = ctx.input.stages.separate;
  if (pipeline === "subtitle" && !sepArgs.always) {
    log("Skipped (subtitle pipeline, set separate.always=true to force)");
    await setStage(taskDir, "separate", {
      status: "success",
      completed_at: nowISO(),
      progress: 100,
      last_message: "Skipped (subtitle pipeline)",
    });
    return;
  }

  await setStage(taskDir, "separate", {
    last_message: "Separating audio...",
    progress: 0,
  });

  const videoPath = ctx.video_source_path!;
  if (!existsSync(videoPath)) throw new Error("video_source.mp4 not found");
  const audioPath = ctx.audioSourcePath!;
  if (!existsSync(audioPath)) throw new Error("audio_source.wav not found");

  if (sepArgs.runtime === "burn") {
    await separateBurn({ taskDir, audioPath, device: sepArgs.device });
  } else if (sepArgs.runtime === "burn-tch") {
    await separateBurn({ taskDir, audioPath, device: sepArgs.device, backend: "tch" });
  }

  await setStage(taskDir, "separate", {
    status: "success",
    completed_at: nowISO(),
    progress: 100,
    last_message: "Separated",
  });
}

// async function separateOrt(taskDir: string, audioPath: string, device: string) {
//   const ep = device === "webgpu" ? "webgpu" : "cpu";
//   const sepCfg = readInputArgs().stages?.separate;
//   const targetStems: Stem[] =
//     sepCfg && "stems" in sepCfg ? ((sepCfg as { stems?: Stem[] }).stems ?? ["vocals"]) : ["vocals"];
//   log(`runtime=ort device=${device} stems=${targetStems.join(",")} → ONNX session(${ep})`);

//   const demucs = new Demucs(undefined, { executionProvider: ep, stems: targetStems });
//   await demucs.load();

//   if (!existsSync(audioPath)) throw new Error("audio_source.wav not found (run download stage)");

//   const t0 = performance.now();
//   const stems = await demucs.separate(audioPath);
//   const elapsedSec = (performance.now() - t0) / 1000;

//   log(`Processed in ${elapsedSec.toFixed(1)}s`);
//   const audioDurationS = stems.vocals.length / 88200;
//   log(`RTF ${(elapsedSec / audioDurationS).toFixed(2)}`);

//   const sepDir = separateDir(taskDir);
//   const stemNames = ["drums", "bass", "other", "vocals"] as const;
//   for (let i = 0; i < stemNames.length; i++) {
//     demucs.writeWav(
//       stems[stemNames[i]],
//       stems.sampleRate,
//       join(sepDir, `target_${i}_${stemNames[i]}.wav`),
//     );
//   }
// }
