//! The 2.9 driver: train on one harvested dataset, eval mAP@0.5 on another
//! (ideally a different APP — held-out pages of the same app flatter the model).
//!
//!   cargo run -p verbivore-grounding --bin train-eval -- <train_dir> <heldout_dir> [epochs]

use verbivore_grounding::data::GroundingDataset;
use verbivore_grounding::eval::evaluate_model;
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
    let acc = evaluate_model(&model, &heldout, &device);
    for (role, gt, ap) in acc.per_class() {
        println!("  class {role:16} gt={gt:5} ap={ap:.3}");
    }
    println!(
        "heldout: mAP@0.5={:.3} matched-IoU={:.3}",
        acc.map50(),
        acc.mean_matched_iou()
    );
    Ok(())
}
