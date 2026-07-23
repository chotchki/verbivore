//! Tune + report the SSIM baseline against a pair dataset:
//!   cargo run --release -p verbivore-effect --bin effect-baseline -- <pairs_dir>

use verbivore_dataset::PairDataset;
use verbivore_effect::tune_ssim_baseline;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("pairs dataset dir");
    let report = tune_ssim_baseline(&PairDataset::open(dir)?)?;
    println!(
        "ssim baseline over {} pairs: threshold={:.4} catch={:.3} false-alarm={:.3} accuracy={:.3} spec-gates={}",
        report.pairs,
        report.threshold,
        report.catch_rate,
        report.false_alarm_rate,
        report.accuracy,
        if report.meets_spec_gates() { "MET" } else { "not met" },
    );
    Ok(())
}
