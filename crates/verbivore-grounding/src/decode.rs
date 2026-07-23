//! Detections out of head tensors: sigmoid -> 3x3 local-max peaks -> boxes,
//! then classic same-class IoU NMS as a belt over the peak-picking suspenders.
//! Runs CPU-side on purpose: decode is authoring-time, clarity beats tensor golf.

use burn::prelude::{Backend, Tensor};
use verbivore_dataset::{Bbox, INTERACTIVE_ROLES};

use crate::data::NUM_CLASSES;
use crate::model::{Detections, OUTPUT_STRIDE};

/// One decoded element in INPUT space (640-square, letterboxed). Map back to
/// screenshot px with `Letterbox::unapply`.
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    /// xyxy, input px.
    pub bbox: [f32; 4],
    pub class: usize,
    pub score: f32,
}

impl Detection {
    pub fn role(&self) -> &'static str {
        INTERACTIVE_ROLES[self.class]
    }
}

pub struct DecodeConfig {
    pub score_threshold: f32,
    pub max_detections: usize,
    pub nms_iou: f32,
}

impl Default for DecodeConfig {
    fn default() -> Self {
        Self {
            score_threshold: 0.3,
            max_detections: 200,
            nms_iou: 0.5,
        }
    }
}

/// Per-image detections for a whole batch.
pub fn decode<B: Backend>(pred: &Detections<B>, cfg: &DecodeConfig) -> Vec<Vec<Detection>> {
    let scores = burn::tensor::activation::sigmoid(pred.heatmap.clone());
    let [batch, classes, grid_h, grid_w] = scores.dims();
    debug_assert_eq!(classes, NUM_CLASSES);

    let heat = to_vec(&scores);
    let sizes = to_vec(&pred.sizes);
    let offsets = to_vec(&pred.offsets);
    let plane = grid_h * grid_w;

    (0..batch)
        .map(|b| {
            let mut dets = Vec::new();
            for c in 0..classes {
                let ch = &heat[(b * classes + c) * plane..(b * classes + c + 1) * plane];
                for y in 0..grid_h {
                    for x in 0..grid_w {
                        let s = ch[y * grid_w + x];
                        if s < cfg.score_threshold || !is_local_max(ch, x, y, grid_w, grid_h) {
                            continue;
                        }
                        let cell = y * grid_w + x;
                        let ox = offsets[(b * 2) * plane + cell];
                        let oy = offsets[(b * 2 + 1) * plane + cell];
                        let w = sizes[(b * 2) * plane + cell].max(0.0);
                        let h = sizes[(b * 2 + 1) * plane + cell].max(0.0);
                        let stride = OUTPUT_STRIDE as f32;
                        let cx = (x as f32 + ox) * stride;
                        let cy = (y as f32 + oy) * stride;
                        dets.push(Detection {
                            bbox: [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0],
                            class: c,
                            score: s,
                        });
                    }
                }
            }
            dets.sort_by(|a, d| d.score.total_cmp(&a.score));
            dets.truncate(cfg.max_detections);
            nms(dets, cfg.nms_iou)
        })
        .collect()
}

fn to_vec<B: Backend, const D: usize>(t: &Tensor<B, D>) -> Vec<f32> {
    t.to_data().to_vec::<f32>().expect("head tensor to host")
}

fn is_local_max(ch: &[f32], x: usize, y: usize, w: usize, h: usize) -> bool {
    let v = ch[y * w + x];
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                // Strict > for neighbors scanned earlier would drop ties twice;
                // >= keeps exactly one of a tied plateau (the first scanned).
                let n = ch[ny as usize * w + nx as usize];
                if n > v || (n == v && (ny as usize * w + nx as usize) < y * w + x) {
                    return false;
                }
            }
        }
    }
    true
}

/// Greedy same-class suppression on a score-sorted list.
fn nms(sorted: Vec<Detection>, iou_threshold: f32) -> Vec<Detection> {
    let mut kept: Vec<Detection> = Vec::with_capacity(sorted.len());
    for det in sorted {
        let overlaps = kept
            .iter()
            .any(|k| k.class == det.class && iou(&k.bbox, &det.bbox) > iou_threshold);
        if !overlaps {
            kept.push(det);
        }
    }
    kept
}

pub fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let ix = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let iy = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let inter = ix * iy;
    let area_a = (a[2] - a[0]).max(0.0) * (a[3] - a[1]).max(0.0);
    let area_b = (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    let union = area_a + area_b - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

/// xyxy input px -> screenshot-space Bbox, the inverse of `Letterbox::apply`.
pub fn unletterbox(lb: &crate::data::Letterbox, bbox: &[f32; 4]) -> Bbox {
    let x0 = (bbox[0] as f64 - lb.pad_x) / lb.scale;
    let y0 = (bbox[1] as f64 - lb.pad_y) / lb.scale;
    let x1 = (bbox[2] as f64 - lb.pad_x) / lb.scale;
    let y1 = (bbox[3] as f64 - lb.pad_y) / lb.scale;
    Bbox {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}
