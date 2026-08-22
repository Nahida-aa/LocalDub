//! Pre-processing for Det / Cls / Rec model inputs.
//!
//! 精确对齐 Python rapidocr：
//! - Det: scale so shortest side is 736px, round to 32px grid (同 C++/Python)
//! - Cls: resize to 48×192, (x/255 - 0.5) / 0.5
//! - Rec: 固定高度 48, 保持纵横比, `img_width = int(48 * ratio)`（无上下限，int 截断）

use crate::image::{resize_bilinear, Image};

pub const DET_LIMIT_SIDE: usize = 736;
pub const CLS_W: usize = 192;
pub const CLS_H: usize = 48;
pub const REC_H: usize = 48;
// 注意：Python 无 REC_MAX_W，此处也不设置

pub struct DetPreproc {
    pub tensor: Vec<f32>, // NCHW, 1 * 3 * resized_h * resized_w
    pub orig_h: usize,
    pub orig_w: usize,
    pub resized_h: usize,
    pub resized_w: usize,
    pub y_offset: usize, // 0 or floor(full_h * 0.6)
}

pub fn preprocess_det(full: &Image, bottom_only: bool) -> DetPreproc {
    let (full_w, full_h) = (full.w, full.h);
    let (y_offset, roi) = if bottom_only {
        let off = ((full_h as f32) * 0.6) as usize;
        let roi_h = full_h - off;
        let mut roi = Image::new(full_w, roi_h);
        for y in 0..roi_h {
            let src_start = ((y + off) * full_w) * 3;
            let dst_start = (y * full_w) * 3;
            roi.data[dst_start..dst_start + full_w * 3]
                .copy_from_slice(&full.data[src_start..src_start + full_w * 3]);
        }
        (off, roi)
    } else {
        (0, full.clone())
    };

    let (orig_w, orig_h) = (roi.w, roi.h);
    let (new_w_f, new_h_f) = if orig_h <= orig_w {
        (
            (orig_w as f32) * (DET_LIMIT_SIDE as f32) / (orig_h as f32),
            DET_LIMIT_SIDE as f32,
        )
    } else {
        (
            DET_LIMIT_SIDE as f32,
            (orig_h as f32) * (DET_LIMIT_SIDE as f32) / (orig_w as f32),
        )
    };
    let new_w = (new_w_f as usize / 32) * 32;
    let new_h = (new_h_f as usize / 32) * 32;

    let mut resized = vec![0u8; new_w * new_h * 3];
    resize_bilinear(&roi.data, roi.w, roi.h, &mut resized, new_w, new_h);

    let mean = [0.485f32, 0.456f32, 0.406f32];
    let std = [0.229f32, 0.224f32, 0.225f32];
    let mut tensor = vec![0.0f32; 3 * new_h * new_w];
    for y in 0..new_h {
        for x in 0..new_w {
            for c in 0..3 {
                let pixel = resized[(y * new_w + x) * 3 + c] as f32;
                tensor[c * new_h * new_w + y * new_w + x] = (pixel / 255.0 - mean[c]) / std[c];
            }
        }
    }

    DetPreproc {
        tensor,
        orig_h,
        orig_w,
        resized_h: new_h,
        resized_w: new_w,
        y_offset,
    }
}

pub fn preprocess_cls(img: &Image) -> Vec<f32> {
    let mut resized = vec![0u8; CLS_W * CLS_H * 3];
    resize_bilinear(&img.data, img.w, img.h, &mut resized, CLS_W, CLS_H);
    let mut tensor = vec![0.0f32; 3 * CLS_H * CLS_W];
    for y in 0..CLS_H {
        for x in 0..CLS_W {
            for c in 0..3 {
                let pixel = resized[(y * CLS_W + x) * 3 + c] as f32;
                tensor[c * CLS_H * CLS_W + y * CLS_W + x] = (pixel / 255.0 - 0.5) / 0.5;
            }
        }
    }
    tensor
}

pub struct RecPreproc {
    pub tensor: Vec<f32>,
    pub width: usize,
}

pub fn preprocess_rec(img: &Image) -> RecPreproc {
    let (orig_w, orig_h) = (img.w as f32, img.h as f32);
    let ratio = orig_w / orig_h;
    // 精确对齐 Python: int(img_height * max_wh_ratio)
    // 在 Rust 中 `as usize` 对正数等同于 int() 截断（向下取整）
    // 注意：Python 没有 min/max 上下限
    let img_w = (REC_H as f32 * ratio) as usize;

    let mut resized = vec![0u8; img_w * REC_H * 3];
    resize_bilinear(&img.data, img.w, img.h, &mut resized, img_w, REC_H);

    let mut tensor = vec![0.0f32; 3 * REC_H * img_w];
    for y in 0..REC_H {
        for x in 0..img_w {
            for c in 0..3 {
                let pixel = resized[(y * img_w + x) * 3 + c] as f32;
                tensor[c * REC_H * img_w + y * img_w + x] = (pixel / 255.0 - 0.5) / 0.5;
            }
        }
    }
    RecPreproc {
        tensor,
        width: img_w,
    }
}
