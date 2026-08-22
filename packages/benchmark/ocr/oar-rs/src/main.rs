use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use oar_ocr::domain::{TextDetectionConfig, TextRecognitionConfig};
use oar_ocr::prelude::*;
use oar_ocr::processors::LimitType;
use serde::Serialize;

#[derive(Serialize)]
struct Segment {
    text: String,
    confidence: f32,
    #[serde(rename = "box")]
    box_: Vec<Vec<f32>>,
}

#[derive(Serialize)]
struct OcrOutput {
    text: String,
    segments: Vec<Segment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    total_ms: f32,
}

fn repo_root() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn models_dir() -> PathBuf {
    if let Ok(dir) = env::var("OCR_MODELS_DIR") {
        PathBuf::from(dir)
    } else {
        repo_root().join("data").join("models").join("rapidocr")
    }
}

fn model_filenames(size: &str) -> (&str, &str, &str) {
    match size {
        "small" => (
            "pp-ocrv6_small_det.onnx",
            "pp-ocrv6_small_rec.onnx",
            "ppocrv6_dict.txt",
        ),
        "medium" => (
            "pp-ocrv6_medium_det.onnx",
            "pp-ocrv6_medium_rec.onnx",
            "ppocrv6_dict.txt",
        ),
        _ => (
            "pp-ocrv6_tiny_det.onnx",
            "pp-ocrv6_tiny_rec.onnx",
            "ppocrv6_tiny_dict.txt",
        ),
    }
}

struct Args {
    target: String,
    dir_mode: bool,
    text_score: f32,
    subtitle_only: bool,
    model_size: String,
    auto_download: bool,
    models_dir: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let args: Vec<String> = env::args().collect();

    let mut a = Args {
        target: String::new(),
        dir_mode: false,
        text_score: 0.5,
        subtitle_only: false,
        model_size: "tiny".to_string(),
        auto_download: false,
        models_dir: models_dir(),
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--subtitle-only" => {
                a.subtitle_only = true;
                i += 1;
            }
            "--dir" => {
                if i + 1 < args.len() {
                    a.target = args[i + 1].clone();
                    a.dir_mode = true;
                    i += 2;
                } else {
                    return Err("--dir requires a directory path".to_string());
                }
            }
            "--model-size" => {
                if i + 1 < args.len() {
                    i += 1;
                    a.model_size = args[i].clone();
                    if !matches!(a.model_size.as_str(), "tiny" | "small" | "medium") {
                        return Err(format!(
                            "--model-size must be tiny, small, or medium (got: {})",
                            a.model_size
                        ));
                    }
                } else {
                    return Err("--model-size requires a value (tiny|small|medium)".to_string());
                }
                i += 1;
            }
            "--auto-download" => {
                a.auto_download = true;
                i += 1;
            }
            s if !s.starts_with("--") => {
                if a.target.is_empty() {
                    a.target = s.to_string();
                } else {
                    a.text_score = s.parse().unwrap_or(0.5);
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    if a.target.is_empty() {
        return Err("No image path or --dir <directory> given".to_string());
    }

    Ok(a)
}

fn list_images(dir: &str) -> Result<Vec<String>, String> {
    let mut files: Vec<String> = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("Cannot read dir: {}", e))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();
        if matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "bmp") {
            files.push(path.to_string_lossy().to_string());
        }
    }
    files.sort();
    Ok(files)
}

fn process_image(
    image_path: &str,
    ocr: &OAROCR,
    text_score: f32,
    subtitle_only: bool,
) -> Result<OcrOutput, String> {
    let t0 = Instant::now();

    let img = image::open(image_path)
        .map_err(|e| format!("Cannot open image {}: {}", image_path, e))?
        .into_rgb8();

    let total_h = img.height() as f32;
    let y_offset = if subtitle_only {
        (total_h * 0.6) as u32
    } else {
        0
    };

    let input = if subtitle_only {
        let (w, h) = (img.width(), img.height());
        let mut img = img;
        image::imageops::crop(&mut img, 0, y_offset, w, h - y_offset).to_image()
    } else {
        img
    };

    let mut results = ocr
        .predict(vec![input])
        .map_err(|e| format!("ocr.predict failed: {}", e))?;

    let total_ms = t0.elapsed().as_secs_f32() * 1000.0;

    let mut segments = Vec::new();
    let mut texts = Vec::new();

    if let Some(result) = results.first_mut() {
        for region in &result.text_regions {
            if let Some((text, conf)) = region.text_with_confidence() {
                if conf < text_score {
                    continue;
                }
                let box_: Vec<Vec<f32>> = region
                    .bounding_box
                    .points
                    .iter()
                    .map(|p| vec![p.x, p.y + y_offset as f32])
                    .collect();
                texts.push(text.to_string());
                segments.push(Segment {
                    text: text.to_string(),
                    confidence: conf,
                    box_,
                });
            }
        }
    }

    Ok(OcrOutput {
        text: texts.join("\n"),
        segments,
        file: None,
        total_ms,
    })
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let md = &args.models_dir;

    let (det_fn, rec_fn, dict_fn) = model_filenames(&args.model_size);

    let ocr = if args.auto_download {
        // Route downloads to models_dir instead of ~/.oar/
        unsafe {
            env::set_var("OAR_HOME", md);
        }

        OAROCRBuilder::new(det_fn, rec_fn, dict_fn)
            .text_detection_config(TextDetectionConfig {
                score_threshold: 0.3,
                box_threshold: 0.5,
                unclip_ratio: 1.6,
                max_candidates: 1000,
                limit_side_len: Some(736),
                limit_type: Some(LimitType::Min),
                max_side_len: None,
            })
            .text_recognition_config(TextRecognitionConfig {
                score_threshold: args.text_score,
                max_text_length: 100,
            })
            .build()
            .map_err(|e| format!("Build OCR (auto-download) failed: {}", e))?
    } else {
        let det_path = md.join(det_fn);
        let rec_path = md.join(rec_fn);
        let dict_path = md.join(dict_fn);
        for p in [&det_path, &rec_path, &dict_path] {
            if !p.exists() {
                return Err(format!(
                    "Model file not found: {} (use --auto-download to download)",
                    p.display()
                ));
            }
        }
        OAROCRBuilder::new(&det_path, &rec_path, &dict_path)
            .text_detection_config(TextDetectionConfig {
                score_threshold: 0.3,
                box_threshold: 0.5,
                unclip_ratio: 1.6,
                max_candidates: 1000,
                limit_side_len: Some(736),
                limit_type: Some(LimitType::Min),
                max_side_len: None,
            })
            .text_recognition_config(TextRecognitionConfig {
                score_threshold: args.text_score,
                max_text_length: 100,
            })
            .build()
            .map_err(|e| format!("Build OCR (local) failed: {}", e))?
    };

    let frame_paths: Vec<String> = if args.dir_mode {
        list_images(&args.target)?
    } else {
        vec![args.target.clone()]
    };

    let mut results: Vec<OcrOutput> = Vec::with_capacity(frame_paths.len());

    for fp in &frame_paths {
        let mut r = process_image(fp, &ocr, args.text_score, args.subtitle_only)?;
        if args.dir_mode {
            r.file = Some(
                std::path::Path::new(fp)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default(),
            );
        }
        results.push(r);
    }

    if args.dir_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&results).map_err(|e| e.to_string())?
        );
    } else if let Some(r) = results.into_iter().next() {
        println!(
            "{}",
            serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?
        );
    }

    Ok(())
}

fn main() {
    if env::var("OAR_DEBUG").is_ok() {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_target(false)
            .init();
    }
    match run() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
