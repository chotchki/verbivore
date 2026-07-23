//! CenterNet target assignment + loss: penalty-reduced focal on the class
//! heatmap, L1 on size/offset at center cells. Targets are built CPU-side in
//! plain Rust (cheap, testable); only the loss itself is tensor math.

use burn::prelude::{Backend, Tensor, TensorData};

use crate::data::NUM_CLASSES;
use crate::model::{Detections, OUTPUT_STRIDE};

/// CenterNet hyperparameters, straight from the paper.
const FOCAL_ALPHA: f32 = 2.0;
const FOCAL_BETA: f32 = 4.0;
const SIZE_WEIGHT: f32 = 0.1;
const EPS: f32 = 1e-4;

/// Dense training targets for one batch.
#[derive(Debug, Clone)]
pub struct Targets<B: Backend> {
    /// [batch, NUM_CLASSES, grid, grid] — gaussian-splatted centers, peak 1.0.
    pub heatmap: Tensor<B, 4>,
    /// [batch, 2, grid, grid] — box w,h in input px, nonzero at center cells.
    pub sizes: Tensor<B, 4>,
    /// [batch, 2, grid, grid] — sub-cell offset in [0,1), nonzero at centers.
    pub offsets: Tensor<B, 4>,
    /// [batch, 1, grid, grid] — 1.0 exactly at center cells.
    pub mask: Tensor<B, 4>,
    /// [batch, 1, grid, grid] — 0.0 where a cell center falls inside an
    /// ignore-region (looks interactive, no label), 1.0 elsewhere. Applied
    /// to the NEGATIVE focal term only: uncertainty is not background, but
    /// real labels near an ignore box keep their positives.
    pub neg_mask: Tensor<B, 4>,
    /// Total center cells across the batch (the focal-loss normalizer).
    pub num_pos: usize,
}

/// Splat radius in cells. Deliberately simpler than the CornerNet quadratic:
/// UI elements are sparse axis-aligned rectangles, overlap barely happens, so
/// a fraction of the smaller box edge is enough. Revisit if eval says otherwise.
fn gaussian_radius(w_cells: f32, h_cells: f32) -> f32 {
    (0.35 * w_cells.min(h_cells)).max(1.0)
}

/// boxes: xyxy in input px, per image; classes parallel; ignore likewise.
/// grid = INPUT/stride.
pub fn build_targets<B: Backend>(
    boxes: &[Vec<[f32; 4]>],
    classes: &[Vec<i64>],
    ignore: &[Vec<[f32; 4]>],
    grid: usize,
    device: &B::Device,
) -> Targets<B> {
    let batch = boxes.len();
    let plane = grid * grid;
    let mut heat = vec![0.0f32; batch * NUM_CLASSES * plane];
    let mut sizes = vec![0.0f32; batch * 2 * plane];
    let mut offsets = vec![0.0f32; batch * 2 * plane];
    let mut mask = vec![0.0f32; batch * plane];
    let mut neg_mask = vec![1.0f32; batch * plane];
    let mut num_pos = 0usize;

    let stride = OUTPUT_STRIDE as f32;
    for (b, img_ignore) in ignore.iter().enumerate() {
        for &[x0, y0, x1, y1] in img_ignore {
            for gy in 0..grid {
                for gx in 0..grid {
                    let (cx, cy) = ((gx as f32 + 0.5) * stride, (gy as f32 + 0.5) * stride);
                    if cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1 {
                        neg_mask[b * plane + gy * grid + gx] = 0.0;
                    }
                }
            }
        }
    }

    for (b, (img_boxes, img_classes)) in boxes.iter().zip(classes).enumerate() {
        for (bx, &class) in img_boxes.iter().zip(img_classes) {
            let [x0, y0, x1, y1] = *bx;
            let (w, h) = (x1 - x0, y1 - y0);
            if w <= 0.0 || h <= 0.0 {
                continue;
            }
            let stride = OUTPUT_STRIDE as f32;
            let (cx, cy) = ((x0 + x1) / 2.0 / stride, (y0 + y1) / 2.0 / stride);
            let (ix, iy) = (cx.floor() as usize, cy.floor() as usize);
            if ix >= grid || iy >= grid || class as usize >= NUM_CLASSES {
                continue;
            }
            num_pos += 1;

            let radius = gaussian_radius(w / stride, h / stride);
            let sigma = radius / 3.0;
            let r = radius.ceil() as isize;
            let channel = class as usize;
            for dy in -r..=r {
                for dx in -r..=r {
                    let (gx, gy) = (ix as isize + dx, iy as isize + dy);
                    if gx < 0 || gy < 0 || gx >= grid as isize || gy >= grid as isize {
                        continue;
                    }
                    let g = (-((dx * dx + dy * dy) as f32) / (2.0 * sigma * sigma)).exp();
                    let idx = ((b * NUM_CLASSES + channel) * grid + gy as usize) * grid
                        + gx as usize;
                    // max, not overwrite: nearby objects keep their own peaks.
                    if g > heat[idx] {
                        heat[idx] = g;
                    }
                }
            }
            // The gaussian may round below 1.0 at the peak; the center is exact.
            heat[((b * NUM_CLASSES + channel) * grid + iy) * grid + ix] = 1.0;

            let cell = iy * grid + ix;
            sizes[(b * 2) * plane + cell] = w;
            sizes[(b * 2 + 1) * plane + cell] = h;
            offsets[(b * 2) * plane + cell] = cx - ix as f32;
            offsets[(b * 2 + 1) * plane + cell] = cy - cy.floor();
            mask[b * plane + cell] = 1.0;
        }
    }

    Targets {
        heatmap: Tensor::from_data(TensorData::new(heat, [batch, NUM_CLASSES, grid, grid]), device),
        sizes: Tensor::from_data(TensorData::new(sizes, [batch, 2, grid, grid]), device),
        offsets: Tensor::from_data(TensorData::new(offsets, [batch, 2, grid, grid]), device),
        mask: Tensor::from_data(TensorData::new(mask, [batch, 1, grid, grid]), device),
        neg_mask: Tensor::from_data(TensorData::new(neg_mask, [batch, 1, grid, grid]), device),
        num_pos,
    }
}

/// Scalar training loss: focal(heatmap) + 0.1 * L1(size) + L1(offset).
pub fn detection_loss<B: Backend>(pred: &Detections<B>, targets: &Targets<B>) -> Tensor<B, 1> {
    let n = targets.num_pos.max(1) as f32;
    let p = burn::tensor::activation::sigmoid(pred.heatmap.clone()).clamp(EPS, 1.0 - EPS);
    let y = targets.heatmap.clone();

    // Positive cells: y == 1 exactly. Everything else is penalty-reduced negative.
    let pos = y.clone().equal_elem(1.0).float();
    let neg = pos.clone().neg() + 1.0;

    let pos_loss = p.clone().neg().add_scalar(1.0).powf_scalar(FOCAL_ALPHA) * p.clone().log() * pos;
    let neg_weight = y.neg().add_scalar(1.0).powf_scalar(FOCAL_BETA);
    // Ignore-regions silence the negative term only: a confident prediction
    // inside one is neither rewarded nor punished.
    let neg_loss = p.clone().powf_scalar(FOCAL_ALPHA)
        * p.neg().add_scalar(1.0).log()
        * neg_weight
        * neg
        * targets.neg_mask.clone().repeat_dim(1, NUM_CLASSES);
    let heat_loss = (pos_loss.sum() + neg_loss.sum()).neg() / n;

    let mask2 = targets.mask.clone().repeat_dim(1, 2);
    let size_loss = ((pred.sizes.clone() - targets.sizes.clone()).abs() * mask2.clone()).sum() / n;
    let offset_loss = ((pred.offsets.clone() - targets.offsets.clone()).abs() * mask2).sum() / n;

    heat_loss + size_loss * SIZE_WEIGHT + offset_loss
}
