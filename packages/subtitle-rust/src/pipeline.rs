//! Full OCR pipeline: load image → preprocess → inference → DB post-process
//! → crop → rec → CTC decode → JSON out.
//!
//! This mirrors the C++ `ocr_pipeline` binary architecture: a single
//! executable runs the entire graph end-to-end, taking the image path on
//! the command line and printing JSON to stdout. Inference calls are
//! ONNX model and returns raw tensor bytes; see `infer.rs` for details.

use std::time::Instant;

use crate::char_list::load_char_list;
use crate::det::{db_postprocess, DetBox};
use crate::image::{order_points_clockwise, rotate_180, warp_perspective_crop, Image, Point};
use crate::infer::load_sessions;
use crate::preprocess::{preprocess_cls, preprocess_det, preprocess_rec};
use crate::rec::ctc_decode;

type Result<T> = std::result::Result<T, String>;

#[derive(serde::Serialize)]
pub struct Segment {
    pub text: String,
    pub confidence: f32,
    #[serde(rename = "box")]
    pub box_: [[f32; 2]; 4],
}

#[derive(serde::Serialize)]
pub struct OcrOutput {
    pub text: String,
    pub segments: Vec<Segment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub char_list_load_ms: f32,
    pub image_load_ms: f32,
    pub model_load_ms: f32,
    pub det_inference_ms: f32,
    pub postprocess_ms: f32,
    pub rec_inference_ms: f32,
    pub total_ms: f32,
}

pub fn run_ocr(
    image_path: &str,
    models_dir: &str,
    keys_path: &str,
    text_score: f32,
    subtitle_only: bool,
) -> Result<OcrOutput> {
    let t_start = Instant::now();

    let t0 = Instant::now();
    let char_list = load_char_list(keys_path)?;
    let char_list_load_ms = t0.elapsed().as_secs_f32() * 1000.0;

    let t0 = Instant::now();
    let mut sessions = load_sessions(models_dir)?;
    let model_load_ms = t0.elapsed().as_secs_f32() * 1000.0;

    let result = run_ocr_core(
        image_path,
        &char_list,
        &mut sessions,
        text_score,
        subtitle_only,
    )?;

    Ok(OcrOutput {
        text: result.text,
        segments: result.segments,
        file: None,
        char_list_load_ms,
        image_load_ms: result.image_load_ms,
        model_load_ms,
        det_inference_ms: result.det_ms,
        postprocess_ms: result.post_ms,
        rec_inference_ms: result.rec_ms,
        total_ms: t_start.elapsed().as_secs_f32() * 1000.0,
    })
}

pub fn run_ocr_with_sessions(
    image_path: &str,
    char_list: &[String],
    sessions: &mut crate::infer::OcrSessions,
    text_score: f32,
    subtitle_only: bool,
    char_list_load_ms_val: f32,
    model_load_ms_val: f32,
) -> Result<OcrOutput> {
    let t_start = Instant::now();
    let result = run_ocr_core(image_path, char_list, sessions, text_score, subtitle_only)?;

    Ok(OcrOutput {
        text: result.text,
        segments: result.segments,
        file: None,
        char_list_load_ms: char_list_load_ms_val,
        image_load_ms: result.image_load_ms,
        model_load_ms: model_load_ms_val,
        det_inference_ms: result.det_ms,
        postprocess_ms: result.post_ms,
        rec_inference_ms: result.rec_ms,
        total_ms: t_start.elapsed().as_secs_f32() * 1000.0,
    })
}

struct CoreResult {
    text: String,
    segments: Vec<Segment>,
    image_load_ms: f32,
    det_ms: f32,
    post_ms: f32,
    rec_ms: f32,
}

fn run_ocr_core(
    image_path: &str,
    char_list: &[String],
    sessions: &mut crate::infer::OcrSessions,
    text_score: f32,
    subtitle_only: bool,
) -> Result<CoreResult> {
    // --- Image load ---
    let t0 = Instant::now();
    let img = Image::load(image_path)?;
    let (full_w, full_h) = (img.w, img.h);
    let image_load_ms = t0.elapsed().as_secs_f32() * 1000.0;

    // --- DET ---
    let t0 = Instant::now();
    let det_prep = preprocess_det(&img, subtitle_only);
    let (hm_w, hm_h) = (det_prep.resized_w, det_prep.resized_h);
    let heatmap = sessions.run_det(&det_prep.tensor, hm_h, hm_w)?;
    let det_ms = t0.elapsed().as_secs_f32() * 1000.0;

    // --- POST ---
    let t0 = Instant::now();
    let mut boxes: Vec<DetBox> = db_postprocess(
        &heatmap,
        hm_w,
        hm_h,
        det_prep.orig_w,
        det_prep.orig_h,
        text_score,
    );
    if det_prep.y_offset > 0 {
        let off = det_prep.y_offset as f32;
        for b in &mut boxes {
            for p in &mut b.polygon {
                p.y += off;
            }
        }
    }
    let post_ms = t0.elapsed().as_secs_f32() * 1000.0;

    // --- CLS + REC ---
    let t0 = Instant::now();
    let mut segs: Vec<Segment> = Vec::new();
    for b in &boxes {
        if subtitle_only {
            let cy = b.polygon.iter().map(|p| p.y).sum::<f32>() / 4.0;
            if cy < 620.0 || cy > 700.0 {
                continue;
            }
        }

        let ordered = order_points_clockwise(&b.polygon);
        let mut clipped = [Point { x: 0.0, y: 0.0 }; 4];
        for (dst, p) in clipped.iter_mut().zip(ordered.iter()) {
            dst.x = p.x.clamp(0.0, (full_w - 1) as f32);
            dst.y = p.y.clamp(0.0, (full_h - 1) as f32);
        }

        let crop = warp_perspective_crop(&img, &clipped);
        if crop.w < 4 || crop.h < 4 {
            continue;
        }

        let cls_tensor = preprocess_cls(&crop);
        let cls_out = sessions.run_cls(&cls_tensor)?;
        let rotate = cls_out.len() >= 2 && cls_out[1] > cls_out[0];
        let rec_crop = if rotate { rotate_180(&crop) } else { crop };

        let rec_prep = preprocess_rec(&rec_crop);
        let rec_out = sessions.run_rec(&rec_prep.tensor, rec_prep.width)?;

        let shape = infer_rec_shape(&rec_out, rec_prep.width);
        let (text, conf) = ctc_decode(&rec_out, &shape, &char_list);
        if text.is_empty() {
            continue;
        }
        if conf < text_score {
            continue;
        }

        let mut box_out = [[0.0f32; 2]; 4];
        for (i, p) in ordered.iter().enumerate() {
            box_out[i] = [p.x.round(), p.y.round()];
        }
        segs.push(Segment {
            text,
            confidence: conf,
            box_: box_out,
        });
    }
    let rec_ms = t0.elapsed().as_secs_f32() * 1000.0;

    // Sort top-to-bottom, left-to-right by bbox center.
    segs.sort_by(|a, b| {
        let ay = a.box_.iter().map(|p| p[1]).sum::<f32>() / 4.0;
        let by = b.box_.iter().map(|p| p[1]).sum::<f32>() / 4.0;
        if (ay - by).abs() > 20.0 {
            ay.partial_cmp(&by).unwrap_or(std::cmp::Ordering::Equal)
        } else {
            let ax = a.box_.iter().map(|p| p[0]).sum::<f32>() / 4.0;
            let bx = b.box_.iter().map(|p| p[0]).sum::<f32>() / 4.0;
            ax.partial_cmp(&bx).unwrap_or(std::cmp::Ordering::Equal)
        }
    });

    let full_text: String = segs.iter().map(|s| s.text.as_str()).collect();

    Ok(CoreResult {
        text: full_text,
        segments: segs,
        image_load_ms,
        det_ms,
        post_ms,
        rec_ms,
    })
}

fn infer_rec_shape(rec_out: &[f32], _width: usize) -> Vec<usize> {
    // PP-OCRv3_rec downsamples by ~8 along the time axis. The number of
    // classes (6625) is fixed for the bundled model; derive timesteps from
    // the total output size.
    let nc = 6625usize;
    let ts = (rec_out.len() / nc).max(1);
    vec![1, ts, nc]
}
