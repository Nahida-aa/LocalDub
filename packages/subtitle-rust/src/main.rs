use std::env;
use std::fs;
use std::path::Path;

mod char_list;
mod det;
mod image;
mod infer;
mod pipeline;
mod preprocess;
mod rec;

use crate::pipeline::{OcrOutput, run_ocr, run_ocr_with_sessions};

fn parse_args() -> Result<(String, f32, bool, String, String, bool), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        return Err(format!(
            "Usage: {} <image_path|--dir <directory>> [text_score] [--subtitle-only] [--device cpu|cuda|dml]",
            args[0]
        ));
    }
    let mut target = String::new();
    let mut dir_mode = false;
    let mut text_score: f32 = 0.5;
    let mut subtitle_only = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--subtitle-only" => {
                subtitle_only = true;
                i += 1;
            }
            "--device" => {
                i += 2;
            }
            "--dir" => {
                if i + 1 < args.len() {
                    target = args[i + 1].clone();
                    dir_mode = true;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            s if !s.starts_with("--") => {
                if target.is_empty() {
                    target = s.to_string();
                } else {
                    text_score = s.parse().unwrap_or(0.5);
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    if target.is_empty() {
        return Err("No image path or --dir <directory> given".to_string());
    }

    let models_dir = env::var("OCR_MODELS_DIR")
        .or_else(|_| env::var("OCR_MODELS"))
        .map_err(|_| "OCR_MODELS_DIR not set")?;
    let keys_path = env::var("OCR_KEYS_PATH").map_err(|_| "OCR_KEYS_PATH not set")?;

    Ok((
        target,
        text_score,
        subtitle_only,
        models_dir,
        keys_path,
        dir_mode,
    ))
}

fn list_images(dir: &str) -> Result<Vec<String>, String> {
    let p = Path::new(dir);
    let mut files: Vec<String> = Vec::new();
    for entry in fs::read_dir(p).map_err(|e| e.to_string())? {
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
        if ext == "jpg" || ext == "jpeg" || ext == "png" || ext == "bmp" {
            files.push(path.to_string_lossy().to_string());
        }
    }
    files.sort();
    Ok(files)
}

fn with_filename(mut result: OcrOutput, filepath: &str) -> OcrOutput {
    result.file = Some(
        Path::new(filepath)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default(),
    );
    result
}

fn main() {
    match (|| -> Result<(), String> {
        let (target, score, so, models, keys, dir_mode) = parse_args()?;

        let frame_paths: Vec<String> = if dir_mode {
            list_images(&target)?
        } else {
            vec![target.clone()]
        };

        let mut results: Vec<OcrOutput> = Vec::with_capacity(frame_paths.len());

        if dir_mode && frame_paths.len() > 1 {
            // Load models once, process all frames
            let t0 = std::time::Instant::now();
            let char_list = crate::char_list::load_char_list(&keys)?;
            let char_list_load_ms = t0.elapsed().as_secs_f32() * 1000.0;

            let t0 = std::time::Instant::now();
            let mut sessions = crate::infer::load_sessions(&models)?;
            let model_load_ms = t0.elapsed().as_secs_f32() * 1000.0;

            for fp in &frame_paths {
                let r = run_ocr_with_sessions(
                    fp,
                    &char_list,
                    &mut sessions,
                    score,
                    so,
                    char_list_load_ms,
                    model_load_ms,
                )?;
                results.push(with_filename(r, fp));
            }
        } else {
            // Single frame: use the convenience function
            let r = run_ocr(&frame_paths[0], &models, &keys, score, so)?;
            results.push(with_filename(r, &frame_paths[0]));
        }

        if dir_mode {
            let json = serde_json::to_string_pretty(&results).map_err(|e| e.to_string())?;
            println!("{}", json);
        } else {
            let json = serde_json::to_string_pretty(&results[0]).map_err(|e| e.to_string())?;
            println!("{}", json);
        }
        Ok(())
    })() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
