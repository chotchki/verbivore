use burn::prelude::*;
use verbivore_dataset::Bbox;
use verbivore_grounding::data::{Letterbox, NUM_CLASSES};
use verbivore_grounding::decode::{DecodeConfig, decode, iou, unletterbox};
use verbivore_grounding::model::Detections;

type B = burn::backend::NdArray<f32>;

const GRID: usize = 16;

/// Hand-built heads: one strong peak for class 0 at cell (4,4) with a weaker
/// shoulder next to it, plus a second peak for class 1 at (10,10).
fn synthetic_heads(device: &Device<B>) -> Detections<B> {
    let plane = GRID * GRID;
    let mut heat = vec![-10.0f32; NUM_CLASSES * plane];
    heat[4 * GRID + 4] = 5.0; // class 0 peak, sigmoid ~0.993
    heat[4 * GRID + 5] = 2.0; // shoulder: above threshold but not a local max
    heat[plane + 10 * GRID + 10] = 3.0; // class 1 peak

    let mut sizes = vec![0.0f32; 2 * plane];
    let mut offsets = vec![0.0f32; 2 * plane];
    for cell in [4 * GRID + 4, 10 * GRID + 10] {
        sizes[cell] = 32.0; // w
        sizes[plane + cell] = 16.0; // h
        offsets[cell] = 0.5;
        offsets[plane + cell] = 0.5;
    }

    Detections {
        heatmap: Tensor::from_data(
            burn::tensor::TensorData::new(heat, [1, NUM_CLASSES, GRID, GRID]),
            device,
        ),
        sizes: Tensor::from_data(burn::tensor::TensorData::new(sizes, [1, 2, GRID, GRID]), device),
        offsets: Tensor::from_data(
            burn::tensor::TensorData::new(offsets, [1, 2, GRID, GRID]),
            device,
        ),
    }
}

#[test]
fn decodes_peaks_suppresses_shoulders() {
    let device = Default::default();
    let dets = decode(&synthetic_heads(&device), &DecodeConfig::default());
    assert_eq!(dets.len(), 1, "one image");
    let dets = &dets[0];
    assert_eq!(dets.len(), 2, "two peaks, shoulder suppressed: {dets:?}");

    let d0 = dets.iter().find(|d| d.class == 0).expect("class 0 peak");
    // Cell (4,4) + offset 0.5 at stride 4 -> center (18,18), box 32x16.
    assert_eq!(d0.bbox, [2.0, 10.0, 34.0, 26.0]);
    assert_eq!(d0.role(), "button");
    assert!(d0.score > 0.99);

    let d1 = dets.iter().find(|d| d.class == 1).expect("class 1 peak");
    assert_eq!(d1.role(), "link");
}

#[test]
fn iou_and_unletterbox_math() {
    assert_eq!(iou(&[0.0, 0.0, 10.0, 10.0], &[0.0, 0.0, 10.0, 10.0]), 1.0);
    assert_eq!(iou(&[0.0, 0.0, 10.0, 10.0], &[20.0, 20.0, 30.0, 30.0]), 0.0);
    let half = iou(&[0.0, 0.0, 10.0, 10.0], &[0.0, 5.0, 10.0, 15.0]);
    assert!((half - 1.0 / 3.0).abs() < 1e-6, "got {half}");

    // Round trip: screenshot bbox -> letterboxed -> back.
    let lb = Letterbox::fit(200, 100);
    let original = Bbox {
        x: 50.0,
        y: 25.0,
        w: 100.0,
        h: 50.0,
    };
    let round_tripped = unletterbox(&lb, &lb.apply(original));
    assert!((round_tripped.x - original.x).abs() < 1e-3);
    assert!((round_tripped.y - original.y).abs() < 1e-3);
    assert!((round_tripped.w - original.w).abs() < 1e-3);
    assert!((round_tripped.h - original.h).abs() < 1e-3);
}

#[test]
fn nms_keeps_the_stronger_of_overlapping_same_class_peaks() {
    let device = Default::default();
    let plane = GRID * GRID;
    // Two class-0 peaks two cells apart with big boxes -> heavy overlap.
    let mut heat = vec![-10.0f32; NUM_CLASSES * plane];
    heat[4 * GRID + 4] = 5.0;
    heat[4 * GRID + 6] = 4.0;
    let mut sizes = vec![0.0f32; 2 * plane];
    for cell in [4 * GRID + 4, 4 * GRID + 6] {
        sizes[cell] = 40.0;
        sizes[plane + cell] = 40.0;
    }
    let pred = Detections::<B> {
        heatmap: Tensor::from_data(
            burn::tensor::TensorData::new(heat, [1, NUM_CLASSES, GRID, GRID]),
            &device,
        ),
        sizes: Tensor::from_data(
            burn::tensor::TensorData::new(sizes, [1, 2, GRID, GRID]),
            &device,
        ),
        offsets: Tensor::zeros([1, 2, GRID, GRID], &device),
    };
    let dets = decode(&pred, &DecodeConfig::default());
    assert_eq!(dets[0].len(), 1, "weaker overlapping peak suppressed");
    assert!(dets[0][0].score > 0.99);
}
