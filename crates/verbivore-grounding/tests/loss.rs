use burn::backend::Autodiff;
use burn::prelude::*;
use verbivore_grounding::loss::{Targets, build_targets, detection_loss};
use verbivore_grounding::model::Detections;

type B = burn::backend::NdArray<f32>;
type AB = Autodiff<B>;

const GRID: usize = 16;

/// One 16x16px box centered at input px (16,16) -> grid cell (4,4) at stride 4.
fn one_box_targets<Back: Backend>(device: &Back::Device) -> Targets<Back> {
    build_targets(
        &[vec![[8.0, 8.0, 24.0, 24.0]]],
        &[vec![0]],
        &[vec![]],
        GRID,
        device,
    )
}

#[test]
fn targets_put_peak_size_and_offset_at_the_center_cell() {
    let device = Default::default();
    let t: Targets<B> = one_box_targets(&device);

    assert_eq!(t.num_pos, 1);
    let heat = t.heatmap.to_data().to_vec::<f32>().unwrap();
    let at = |c: usize, y: usize, x: usize| heat[(c * GRID + y) * GRID + x];
    assert_eq!(at(0, 4, 4), 1.0, "exact 1.0 at the center");
    assert!(at(0, 4, 5) > 0.0 && at(0, 4, 5) < 1.0, "gaussian shoulder");
    assert_eq!(at(1, 4, 4), 0.0, "other classes untouched");

    let sizes = t.sizes.to_data().to_vec::<f32>().unwrap();
    let plane = GRID * GRID;
    assert_eq!(sizes[4 * GRID + 4], 16.0, "w at center");
    assert_eq!(sizes[plane + 4 * GRID + 4], 16.0, "h at center");

    let mask_sum: f32 = t.mask.to_data().to_vec::<f32>().unwrap().iter().sum();
    assert_eq!(mask_sum, 1.0);
}

fn perfect_prediction<Back: Backend>(t: &Targets<Back>) -> Detections<Back> {
    // +10 logits where target is exactly 1, -10 elsewhere: sigmoid lands on
    // ~1/~0 which is as close to the target peaks as logits can express.
    let pos = t.heatmap.clone().equal_elem(1.0).float();
    Detections {
        heatmap: pos * 20.0 - 10.0,
        sizes: t.sizes.clone(),
        offsets: t.offsets.clone(),
    }
}

#[test]
fn loss_is_near_zero_for_perfect_and_large_for_wrong() {
    let device = Default::default();
    let t: Targets<B> = one_box_targets(&device);

    let perfect = detection_loss(&perfect_prediction(&t), &t)
        .into_scalar();
    assert!(perfect < 0.05, "perfect prediction should be ~free: {perfect}");

    let wrong = Detections {
        // "Everything everywhere is a button": max punishment from the negatives.
        heatmap: t.heatmap.ones_like() * 10.0,
        sizes: t.sizes.zeros_like(),
        offsets: t.offsets.zeros_like(),
    };
    let wrong = detection_loss(&wrong, &t).into_scalar();
    assert!(
        wrong > perfect * 100.0,
        "wrong ({wrong}) should dwarf perfect ({perfect})"
    );
}

#[test]
fn loss_backpropagates() {
    let device = Default::default();
    let t: Targets<AB> = one_box_targets(&device);
    let heatmap = Tensor::<AB, 4>::random(
        [1, verbivore_grounding::data::NUM_CLASSES, GRID, GRID],
        burn::tensor::Distribution::Normal(0.0, 1.0),
        &device,
    )
    .require_grad();
    let pred = Detections {
        heatmap: heatmap.clone(),
        sizes: t.sizes.clone().require_grad(),
        offsets: t.offsets.clone().require_grad(),
    };
    let loss = detection_loss(&pred, &t);
    let grads = loss.backward();
    assert!(
        heatmap.grad(&grads).is_some(),
        "heatmap must receive gradients"
    );
}

#[test]
fn ignore_region_silences_negatives_but_not_positives() {
    let device = Default::default();
    // Same single box, plus an ignore-region over cells ~(8..12, 8..12) —
    // far from the labeled object at cell (4,4).
    let with_ignore: Targets<B> = build_targets(
        &[vec![[8.0, 8.0, 24.0, 24.0]]],
        &[vec![0]],
        &[vec![[32.0, 32.0, 48.0, 48.0]]],
        GRID,
        &device,
    );
    let without: Targets<B> = one_box_targets(&device);

    // A model screaming "button!" inside the ignore box: punished without the
    // mask, free with it.
    let mut confident = without.heatmap.zeros_like().to_data().to_vec::<f32>().unwrap();
    for y in 8..12 {
        for x in 8..12 {
            confident[y * GRID + x] = 10.0; // class 0 plane
        }
    }
    let pred = |t: &Targets<B>| Detections {
        heatmap: Tensor::from_data(
            burn::tensor::TensorData::new(
                confident.clone(),
                [1, verbivore_grounding::data::NUM_CLASSES, GRID, GRID],
            ),
            &device,
        ) + perfect_prediction(t).heatmap,
        sizes: t.sizes.clone(),
        offsets: t.offsets.clone(),
    };
    let masked = detection_loss(&pred(&with_ignore), &with_ignore).into_scalar();
    let unmasked = detection_loss(&pred(&without), &without).into_scalar();
    assert!(
        masked < unmasked / 10.0,
        "ignore must silence the negative loss there: masked={masked} unmasked={unmasked}"
    );

    // The positive at (4,4) still teaches: a miss there must still hurt.
    let blind = Detections {
        heatmap: with_ignore.heatmap.zeros_like() - 10.0,
        sizes: with_ignore.sizes.clone(),
        offsets: with_ignore.offsets.clone(),
    };
    let miss = detection_loss(&blind, &with_ignore).into_scalar();
    assert!(miss > 1.0, "missing the labeled object must still cost: {miss}");
}
