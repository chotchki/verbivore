//! The 2.9 driver: train on one harvested dataset, eval mAP@0.5 on another
//! (ideally a different APP — held-out pages of the same app flatter the model).
//!
//!   cargo run -p verbivore-grounding --bin train-eval -- <train_dir> <heldout_dir> [epochs]

use burn::data::dataloader::batcher::Batcher;
use burn::data::dataset::Dataset as BurnDataset;
use verbivore_grounding::data::{GroundingBatcher, GroundingDataset};
use verbivore_grounding::decode::{DecodeConfig, decode};
use verbivore_grounding::eval::EvalAccumulator;
use verbivore_grounding::train::{TrainConfig, train, valid_model};

type AB = burn::backend::Autodiff<burn::backend::Wgpu>;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let train_dir = args.next().expect("train dataset dir");
    let heldout_dir = args.next().expect("heldout dataset dir");
    let epochs: usize = args.next().map(|a| a.parse()).transpose()?.unwrap_or(60);

    let device = Default::default();
    let config = TrainConfig {
        epochs,
        batch_size: 8,
        checkpoint_dir: Some(std::path::PathBuf::from("target/train-eval-ckpt")),
        ..TrainConfig::default()
    };
    let outcome = train::<AB>(&config, GroundingDataset::open_cached(&train_dir)?, &device)?;
    let model = valid_model(&outcome.model);

    let heldout = GroundingDataset::open(&heldout_dir)?;
    let mut acc = EvalAccumulator::default();
    // Eval decodes near-zero threshold: mAP judges the full ranking, and the
    // runtime default (0.3) would truncate the PR curve before it's measured.
    let decode_cfg = DecodeConfig {
        score_threshold: 0.05,
        max_detections: 300,
        ..DecodeConfig::default()
    };
    let mut buffered = Vec::new();
    let flush = |items: &mut Vec<_>, acc: &mut EvalAccumulator| {
        if items.is_empty() {
            return;
        }
        let batch = GroundingBatcher.batch(std::mem::take(items), &device);
        let dets = decode(&model.forward(batch.images), &decode_cfg);
        for ((dets, gt_boxes), gt_classes) in dets.iter().zip(&batch.boxes).zip(&batch.classes) {
            acc.observe(dets, gt_boxes, gt_classes);
        }
    };
    for i in 0..BurnDataset::len(&heldout) {
        buffered.push(BurnDataset::get(&heldout, i).unwrap());
        if buffered.len() == 8 {
            flush(&mut buffered, &mut acc);
        }
    }
    flush(&mut buffered, &mut acc);

    println!(
        "heldout: mAP@0.5={:.3} matched-IoU={:.3}",
        acc.map50(),
        acc.mean_matched_iou()
    );
    Ok(())
}
