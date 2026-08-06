//! DB (Differentiable Binarization) post-processing.
//!
//! Pipeline (matches the C++ implementation):
//!   1. Apply sigmoid (if values are logits) → probability map.
//!   2. `prob[i] > thr` → 8-bit bitmap.
//!   3. 2×2 dilate to reconnect thin regions (matches Python `use_dilation`).
//!   4. Connected components via cv::findContours.
//!   5. For each component: cv::minAreaRect → unclip → score.
//!   6. Drop boxes with score < box_threshold.
//!
//! Performance: the OpenCV `imgproc` routines are SIMD-optimized and much
//! faster than our hand-written connected_components + convex_hull.

use std::ffi::c_void;

use crate::image::Point;

use opencv::core::{Mat, Point as CvPoint, Point2f, Size, Vector};
use opencv::imgproc;

const DET_THRESH: f32 = 0.3;
const UNCLIP_RATIO: f32 = 1.6;
const MAX_CANDIDATES: usize = 1000;

pub struct DetBox {
    pub polygon: [Point; 4],
    pub score: f32,
}

pub fn db_postprocess(
    heatmap: &[f32],
    hm_w: usize,
    hm_h: usize,
    orig_w: usize,
    orig_h: usize,
    box_thresh: f32,
) -> Vec<DetBox> {
    let sigmoid = heatmap.iter().any(|&v| v > 1.0);
    let prob: Vec<f32> = if sigmoid {
        heatmap.iter().map(|&v| 1.0 / (1.0 + (-v).exp())).collect()
    } else {
        heatmap.to_vec()
    };

    let mut bitmap_data = vec![0u8; hm_w * hm_h];
    for i in 0..hm_w * hm_h {
        if prob[i] > DET_THRESH {
            bitmap_data[i] = 255;
        }
    }

    let bitmap_mat = unsafe {
        Mat::new_rows_cols_with_data_unsafe_def(
            hm_h as i32,
            hm_w as i32,
            opencv::core::CV_8U,
            bitmap_data.as_mut_ptr() as *mut c_void,
        )
        .expect("cv::Mat")
    };

    let mut dilated = Mat::default();
    let kernel = imgproc::get_structuring_element_def(imgproc::MORPH_RECT, Size::new(2, 2))
        .expect("cv::kernel");
    imgproc::dilate_def(&bitmap_mat, &mut dilated, &kernel).expect("cv::dilate");

    let mut contours: Vector<Vector<CvPoint>> = Vector::new();
    imgproc::find_contours_def(
        &dilated,
        &mut contours,
        imgproc::RETR_LIST,
        imgproc::CHAIN_APPROX_SIMPLE,
    )
    .expect("cv::findContours");

    let mut out: Vec<DetBox> = Vec::new();
    let n_contours = contours.len();
    for ci in 0..n_contours {
        let pts_vec = contours.get(ci).expect("contour");
        if pts_vec.len() < 3 {
            continue;
        }

        let pts_2f_vec: Vector<Point2f> = pts_vec
            .iter()
            .map(|p| Point2f::new(p.x as f32, p.y as f32))
            .collect();

        let rect = imgproc::min_area_rect(&pts_2f_vec).expect("cv::minAreaRect");
        let width = rect.size.width;
        let height = rect.size.height;
        let short = width.min(height);
        if short < 3.0 {
            continue;
        }

        let score = {
            let (cx, cy) = (rect.center.x, rect.center.y);
            let angle_rad = rect.angle.to_radians();
            let (cos_a, sin_a) = (angle_rad.cos(), angle_rad.sin());
            let hw = width * 0.5;
            let hh = height * 0.5;

            let pts_corners = cv_box_points_f32(&rect);
            let xmin = pts_corners
                .iter()
                .map(|p| p.0)
                .fold(f32::INFINITY, f32::min)
                .floor() as i32;
            let xmax = pts_corners
                .iter()
                .map(|p| p.0)
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil() as i32;
            let ymin = pts_corners
                .iter()
                .map(|p| p.1)
                .fold(f32::INFINITY, f32::min)
                .floor() as i32;
            let ymax = pts_corners
                .iter()
                .map(|p| p.1)
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil() as i32;

            let mut sum = 0.0f64;
            let mut count: i64 = 0;
            for y in ymin..=ymax {
                for x in xmin..=xmax {
                    let dx = (x as f32) - cx;
                    let dy = (y as f32) - cy;
                    let lx = dx * cos_a + dy * sin_a;
                    let ly = -dx * sin_a + dy * cos_a;
                    if lx < -hw || lx > hw || ly < -hh || ly > hh {
                        continue;
                    }
                    let xi = x.clamp(0, hm_w as i32 - 1) as usize;
                    let yi = y.clamp(0, hm_h as i32 - 1) as usize;
                    sum += prob[yi * hm_w + xi] as f64;
                    count += 1;
                }
            }
            if count == 0 {
                0.0
            } else {
                (sum / count as f64) as f32
            }
        };
        if score < box_thresh {
            continue;
        }

        let dist = (width * height * UNCLIP_RATIO) / (2.0 * (width + height));
        let dist = dist.max(3.0);
        let unclip_w = width + 2.0 * dist;
        let unclip_h = height + 2.0 * dist;
        let sx = orig_w as f32 / hm_w as f32;
        let sy = orig_h as f32 / hm_h as f32;
        let a = rect.angle.to_radians();
        let (cos_a, sin_a) = (a.cos(), a.sin());
        let hw2 = unclip_w * 0.5;
        let hh2 = unclip_h * 0.5;
        let (bcx, bcy) = (rect.center.x, rect.center.y);
        let local = [(-hw2, -hh2), (hw2, -hh2), (hw2, hh2), (-hw2, hh2)];
        let mut pts = [Point { x: 0.0, y: 0.0 }; 4];
        for i in 0..4 {
            let (lx, ly) = local[i];
            let rx = lx * cos_a - ly * sin_a + bcx;
            let ry = lx * sin_a + ly * cos_a + bcy;
            pts[i] = Point {
                x: (rx * sx).clamp(0.0, (orig_w - 1) as f32),
                y: (ry * sy).clamp(0.0, (orig_h - 1) as f32),
            };
        }
        out.push(DetBox {
            polygon: pts,
            score,
        });
    }
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(MAX_CANDIDATES);
    out
}

fn cv_box_points_f32(rect: &opencv::core::RotatedRect) -> Vec<(f32, f32)> {
    let angle = rect.angle.to_radians();
    let (cos_a, sin_a) = (angle.cos(), angle.sin());
    let hw = rect.size.width * 0.5;
    let hh = rect.size.height * 0.5;
    let (cx, cy) = (rect.center.x, rect.center.y);
    vec![
        (
            cx + (-hw) * cos_a - (-hh) * sin_a,
            cy + (-hw) * sin_a + (-hh) * cos_a,
        ),
        (
            cx + hw * cos_a - (-hh) * sin_a,
            cy + hw * sin_a + (-hh) * cos_a,
        ),
        (cx + hw * cos_a - hh * sin_a, cy + hw * sin_a + hh * cos_a),
        (
            cx + (-hw) * cos_a - hh * sin_a,
            cy + (-hw) * sin_a + hh * cos_a,
        ),
    ]
}
