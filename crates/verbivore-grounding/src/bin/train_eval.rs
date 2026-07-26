//! The 2.9 driver: train on one harvested dataset, eval mAP@0.5 on another
//! (ideally a different APP — held-out pages of the same app flatter the model).
//!
//!   cargo run -p verbivore-grounding --bin train-eval -- <train_dir> <heldout_dir> [epochs] [seed]

use verbivore_grounding::data::GroundingDataset;
use verbivore_grounding::eval::{PriorCondition, evaluate_model, evaluate_model_under};
use verbivore_grounding::train::{TrainConfig, train, valid_model};

type AB = burn::backend::Autodiff<burn::backend::Wgpu>;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let train_dir = args.next().expect("train dataset dir");
    let heldout_dir = args.next().expect("heldout dataset dir");
    let epochs: usize = args.next().map(|a| a.parse()).transpose()?.unwrap_or(60);
    // Seed drives backend init AND dataloader shuffle; the B.1 variance
    // harness sweeps it to separate seed variance from wgpu nondeterminism.
    let seed: u64 = args.next().map(|a| a.parse()).transpose()?.unwrap_or(42);

    let device = Default::default();
    let config = TrainConfig {
        epochs,
        batch_size: 8,
        seed,
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
    // Size stratification for the two mass classes: the resolution probe.
    for class in [1usize, 0] {
        let role = verbivore_dataset::INTERACTIVE_ROLES[class];
        for ((lo, hi), gt, ap) in acc.class_by_size(class) {
            if gt > 0 {
                println!("  {role:8} h[{lo:>4.0},{hi:>4.0})px gt={gt:5} ap={ap:.3}");
            }
        }
    }
    // C.4 three-condition protocol: flat is the canvas condition AND the
    // shipping gate (must not regress below the pixels-only baseline).
    // Full stays LAST — rotate.sh extracts the summary from the tail line.
    for (name, condition) in [
        ("flat", PriorCondition::Flat),
        ("degraded", PriorCondition::Degraded { seed: seed ^ 0xC4 }),
    ] {
        let c = evaluate_model_under(&model, &heldout, &device, condition);
        println!(
            "heldout[{name}]: mAP@0.5={:.3} matched-IoU={:.3}",
            c.map50(),
            c.mean_matched_iou()
        );
    }
    println!(
        "heldout: mAP@0.5={:.3} matched-IoU={:.3}",
        acc.map50(),
        acc.mean_matched_iou()
    );
    Ok(())
}
