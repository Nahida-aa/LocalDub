//! Image utilities (replacement for C++ image.h).
//!
//! Loads PNG/JPEG via the `image` crate into an in-memory RGB buffer, and
//! provides geometric helpers used by the OCR pipeline: bilinear resize, 180°
//! rotation, and perspective-warp crop (for pulling text regions out of the
//! original frame).
//!
//! Performance: heavy lifting is delegated to OpenCV, which uses SIMD-optimized
//! kernels for resize, flip and perspective warp.

use std::ffi::c_void;
use std::path::Path;

use opencv::core::{CV_8UC, Mat, Point2f, Size};
use opencv::imgproc;
use opencv::prelude::*;

/// 3-channel, 8-bit per channel RGB image. Pixels are laid out row-major.
#[derive(Clone)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    /// Length = `w * h * 3`. Pixel at (x, y) starts at `(y * w + x) * 3` and
    /// is stored as (r, g, b).
    pub data: Vec<u8>,
}

impl Image {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            data: vec![0u8; w * h * 3],
        }
    }

    /// Load from path. Converts any supported input to RGB8, then swaps
    /// to BGR to match Python rapidocr (which uses cv2.imread directly).
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let img = image::open(path.as_ref()).map_err(|e| format!("load image failed: {}", e))?;
        let rgb = img.to_rgb8();
        let (w, h) = (rgb.width() as usize, rgb.height() as usize);
        let mut raw = rgb.into_raw();
        // RGB -> BGR
        for px in raw.chunks_exact_mut(3) {
            px.swap(0, 2);
        }
        Ok(Self { w, h, data: raw })
    }
}

/// Build an OpenCV Mat that wraps our BGR buffer (no copy).
/// Color channel order is BGR (matches rapidocr + cv2.imread).
fn as_mat(img: &Image) -> Mat {
    unsafe {
        Mat::new_rows_cols_with_data_unsafe_def(
            img.h as i32,
            img.w as i32,
            CV_8UC(3),
            img.data.as_ptr() as *mut c_void,
        )
        .expect("cv::Mat wrap")
    }
}

/// Copy data from an OpenCV Mat (8UC3) into a new Image.
/// Uses raw pointer access to handle both continuous and non-continuous Mats.
fn from_mat(mat: &Mat) -> Image {
    let w = mat.cols() as usize;
    let h = mat.rows() as usize;
    let mut out = Image::new(w, h);
    let mat_ptr = mat.data() as *const u8;
    let step = mat.step1(0).unwrap_or(0) as usize;
    let row_bytes = w * 3;
    if mat.is_continuous() && step == row_bytes {
        // Fast path: memcpy the whole buffer
        let total = row_bytes * h;
        unsafe {
            std::ptr::copy_nonoverlapping(mat_ptr, out.data.as_mut_ptr(), total);
        }
    } else {
        // Safe path: copy row by row
        for y in 0..h {
            let src = unsafe { mat_ptr.add(y * step) };
            let dst = out.data.as_mut_ptr() as *mut u8;
            unsafe {
                std::ptr::copy_nonoverlapping(src, dst.add(y * row_bytes), row_bytes);
            }
        }
    }
    out
}

/// Resize src (src_w x src_h) into dst (dst_w x dst_h) using bilinear
/// interpolation, delegated to OpenCV's SIMD-optimized kernel.
pub fn resize_bilinear(
    src: &[u8],
    src_w: usize,
    src_h: usize,
    dst: &mut [u8],
    dst_w: usize,
    dst_h: usize,
) {
    let src_mat = unsafe {
        Mat::new_rows_cols_with_data_unsafe_def(
            src_h as i32,
            src_w as i32,
            CV_8UC(3),
            src.as_ptr() as *mut c_void,
        )
        .expect("cv::Mat src")
    };
    let mut dst_mat = Mat::default();
    imgproc::resize(
        &src_mat,
        &mut dst_mat,
        Size::new(dst_w as i32, dst_h as i32),
        0.0,
        0.0,
        imgproc::INTER_LINEAR,
    )
    .expect("cv::resize");
    let total = dst_w * dst_h * 3;
    let mat_ptr = dst_mat.data() as *const u8;
    let step = dst_mat.step1(0).unwrap_or(0) as usize;
    let row_bytes = dst_w * 3;
    if dst_mat.is_continuous() && step == row_bytes {
        unsafe {
            std::ptr::copy_nonoverlapping(mat_ptr, dst.as_mut_ptr(), total);
        }
    } else {
        for y in 0..dst_h {
            let src = unsafe { mat_ptr.add(y * step) };
            unsafe {
                std::ptr::copy_nonoverlapping(src, dst.as_mut_ptr().add(y * row_bytes), row_bytes);
            }
        }
    }
}

pub fn rotate_180(img: &Image) -> Image {
    let mut out = Image::new(img.w, img.h);
    let half = img.h / 2;
    for y in 0..half {
        let y2 = img.h - 1 - y;
        let row_bytes = img.w * 3;
        let s1 = y * row_bytes;
        let s2 = y2 * row_bytes;
        for x in 0..row_bytes {
            out.data[s1 + x] = img.data[s2 + (row_bytes - 1 - x)];
            out.data[s2 + x] = img.data[s1 + (row_bytes - 1 - x)];
        }
    }
    if img.h % 2 == 1 {
        let y = half;
        let row_bytes = img.w * 3;
        let s = y * row_bytes;
        for x in 0..row_bytes {
            out.data[s + x] = img.data[s + (row_bytes - 1 - x)];
        }
    }
    out
}

/// 2D point with float coordinates (subpixel).
#[derive(Copy, Clone, Debug)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// Perspective-warp crop: maps source 4-point polygon to a rectangle of size
/// (out_w, out_h), pulling pixels from `img` via bilinear sampling. Uses
/// OpenCV's getPerspectiveTransform + warpPerspective for SIMD speed.
pub fn warp_perspective_crop(img: &Image, pts: &[Point; 4]) -> Image {
    let xs: [f32; 4] = [pts[0].x, pts[1].x, pts[2].x, pts[3].x];
    let ys: [f32; 4] = [pts[0].y, pts[1].y, pts[2].y, pts[3].y];
    let w1 = ((xs[1] - xs[0]).powi(2) + (ys[1] - ys[0]).powi(2)).sqrt();
    let w2 = ((xs[2] - xs[3]).powi(2) + (ys[2] - ys[3]).powi(2)).sqrt();
    let h1 = ((xs[3] - xs[0]).powi(2) + (ys[3] - ys[0]).powi(2)).sqrt();
    let h2 = ((xs[2] - xs[1]).powi(2) + (ys[2] - ys[1]).powi(2)).sqrt();

    let dst_w = 4.max(w1.max(w2).round() as usize);
    let dst_h = 4.max(h1.max(h2).round() as usize);
    let rotate90 = (dst_h as f32) / (dst_w as f32) >= 1.5;

    // Output corners: TL, TR, BR, BL — matching the order_points_clockwise
    // convention used to order the source points in pipeline.rs.
    let dst_pts_arr = [
        Point2f::new(0.0, 0.0),
        Point2f::new((dst_w - 1) as f32, 0.0),
        Point2f::new((dst_w - 1) as f32, (dst_h - 1) as f32),
        Point2f::new(0.0, (dst_h - 1) as f32),
    ];
    let src_pts_arr = [
        Point2f::new(pts[0].x, pts[0].y),
        Point2f::new(pts[1].x, pts[1].y),
        Point2f::new(pts[2].x, pts[2].y),
        Point2f::new(pts[3].x, pts[3].y),
    ];

    // OpenCV: getPerspectiveTransform(src, dst) returns M: src->dst.
    // warpPerspective with default flags expects M: src->dst (inverts internally to sample).
    let m = imgproc::get_perspective_transform_slice_def(&src_pts_arr, &dst_pts_arr)
        .expect("cv::getPerspectiveTransform");

    let src = as_mat(img);
    let mut out_mat = Mat::default();
    let out_size = Size::new(dst_w as i32, dst_h as i32);
    imgproc::warp_perspective(
        &src,
        &mut out_mat,
        &m,
        out_size,
        imgproc::INTER_LINEAR,
        0, // border_type = BORDER_CONSTANT
        opencv::core::Scalar::all(0.0),
    )
    .expect("cv::warpPerspective");

    let cropped = from_mat(&out_mat);

    if rotate90 {
        // Rotate 90° clockwise: new width = cropped.h, new height = cropped.w
        let (in_w, in_h) = (cropped.w, cropped.h);
        let mut out = Image::new(in_h, in_w);
        for y in 0..in_h {
            for x in 0..in_w {
                let src_idx = (y * in_w + x) * 3;
                // 90° CW: out[x, in_h - 1 - y] = in[x, y]
                let out_x = in_h - 1 - y;
                let out_y = x;
                let dst_idx = (out_y * in_h + out_x) * 3;
                out.data[dst_idx] = cropped.data[src_idx];
                out.data[dst_idx + 1] = cropped.data[src_idx + 1];
                out.data[dst_idx + 2] = cropped.data[src_idx + 2];
            }
        }
        out
    } else {
        cropped
    }
}

/// Order 4 points clockwise: TL (top-left), TR (top-right), BR (bottom-right),
/// BL (bottom-left). Matches C++ `orderPointsClockwise`.
pub fn order_points_clockwise(pts: &[Point; 4]) -> [Point; 4] {
    let mut idx: [usize; 4] = [0, 1, 2, 3];
    idx.sort_by(|&a, &b| pts[a].x.partial_cmp(&pts[b].x).unwrap());

    let left = [idx[0], idx[1]];
    let mut left_sorted = left;
    left_sorted.sort_by(|&a, &b| pts[a].y.partial_cmp(&pts[b].y).unwrap());
    let tl = left_sorted[0];
    let bl = left_sorted[1];

    let right = [idx[2], idx[3]];
    let mut right_sorted = right;
    right_sorted.sort_by(|&a, &b| pts[a].y.partial_cmp(&pts[b].y).unwrap());
    let tr = right_sorted[0];
    let br = right_sorted[1];

    [pts[tl], pts[tr], pts[br], pts[bl]]
}
