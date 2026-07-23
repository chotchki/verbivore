//! Eval a saved checkpoint against a dataset, no training:
//!   cargo run --release -p verbivore-grounding --bin eval-ckpt -- <ckpt.mpk> <dataset_dir>

use verbivore_grounding::data::GroundingDataset;
use verbivore_grounding::eval::evaluate_model;
use verbivore_grounding::train::load_checkpoint;

type B = burn::backend::Wgpu;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let ckpt = args.next().expect("checkpoint path (.mpk)");
    let dataset_dir = args.next().expect("dataset dir");

    let device = Default::default();
    let model = load_checkpoint::<B>(ckpt.trim_end_matches(".mpk"), &device)?;
    let dataset = GroundingDataset::open(&dataset_dir)?;
    let acc = evaluate_model(&model, &dataset, &device);
    println!(
        "mAP@0.5={:.3} matched-IoU={:.3}",
        acc.map50(),
        acc.mean_matched_iou()
    );
    Ok(())
}
