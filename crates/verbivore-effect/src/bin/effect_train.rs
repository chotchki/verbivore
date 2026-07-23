//! 3.6: train the diff-stack effect model for real. The protocol is threshold
//! TRANSFER — SSIM and the model both tune their operating threshold on TRAIN
//! scores and carry it FROZEN to heldout, because a threshold you can't set
//! without peeking at the eval set is a threshold you can't ship. Oracle
//! numbers print alongside as reference ceilings. The split-composition header
//! is the honesty check: a heldout side with zero ambient-noisy urls cannot
//! stress SSIM, and its verdict means nothing.
//!
//!   cargo run --release -p verbivore-effect --bin effect-train -- \
//!     <pairs_dir> [epochs] [out_dir]
//!
//! Writes `<out_dir>/effect-model.mpk` + `<out_dir>/effect-model.json`
//! (frozen threshold + input dims) — the artifact phase 4's gate loads.

use burn::backend::Autodiff;
use burn::module::AutodiffModule;
use burn::optim::{AdamWConfig, GradientsParams, Optimizer};
use burn::prelude::*;
use burn::record::CompactRecorder;
use verbivore_dataset::PairDataset;
use verbivore_effect::gate::GateSidecar;
use verbivore_effect::models::DiffStackModel;
use verbivore_effect::pair_data::{PAIR_H, PAIR_W, PairBatcher, load_visible_split};
use verbivore_effect::train::{Lcg, bce, best_operating_point, operating_point_at, scores};

use burn::data::dataloader::batcher::Batcher;

type WB = burn::backend::Wgpu;
type AB = Autodiff<WB>;

fn gate(catch: f64, fa: f64) -> &'static str {
    if catch >= 0.95 && fa <= 0.05 { "PASS" } else { "FAIL" }
}

/// Every heldout pair the frozen threshold gets wrong, with provenance —
/// the FA lists for ssim vs model are the diagnostic that matters.
fn dump_misses(scored: &[(f64, bool)], items: &[verbivore_effect::pair_data::PairItem], t: f64) {
    for ((score, _), item) in scored.iter().zip(items) {
        if (*score >= t) != item.changed {
            let kind = if item.changed { "MISS" } else { "FALSE-ALARM" };
            println!("  {kind} score={score:.3} click={:?} {}", item.click, item.url);
        }
    }
}

fn report(name: &str, train: &[(f64, bool)], heldout: &[(f64, bool)]) -> (f64, f64, f64) {
    let (t, tc, tf) = best_operating_point(train);
    let (hc, hf) = operating_point_at(heldout, t);
    let (ot, oc, ofa) = best_operating_point(heldout);
    println!(
        "{name}: frozen t={t:.4} | train {tc:.3}/{tf:.3} | heldout {hc:.3}/{hf:.3} -> gates {} | oracle ceiling {oc:.3}/{ofa:.3} @ {ot:.4}",
        gate(hc, hf)
    );
    (t, hc, hf)
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("pairs dataset dir");
    let epochs: usize = args.next().map(|a| a.parse()).transpose()?.unwrap_or(60);
    let out = std::path::PathBuf::from(
        args.next().unwrap_or_else(|| "effect-checkpoint".to_string()),
    );

    let split = load_visible_split(&PairDataset::open(dir)?)?;
    print!("{}", split.composition);
    if split.composition.heldout.noisy_urls == 0 {
        println!("WARNING: no ambient-noisy urls in heldout — this eval cannot stress ssim");
    }

    // SSIM baseline, same transfer protocol. Score = 1 - mssim (higher = changed).
    let flip = |v: &[(f64, bool)]| -> Vec<(f64, bool)> {
        v.iter().map(|(s, c)| (1.0 - s, *c)).collect()
    };
    let heldout_ssim = flip(&split.heldout_ssim);
    let (ssim_t, _, _) = report("ssim", &flip(&split.train_ssim), &heldout_ssim);
    dump_misses(&heldout_ssim, &split.heldout, ssim_t);

    let device: Device<AB> = Default::default();
    let mut model = DiffStackModel::<AB>::init(&device);
    let mut optim = AdamWConfig::new().init();
    let mut rng = Lcg(7);
    for epoch in 1..=epochs {
        let mut order: Vec<usize> = (0..split.train.len()).collect();
        rng.shuffle(&mut order);
        let mut loss_sum = 0.0;
        let mut batches = 0;
        for chunk in order.chunks(16) {
            let items = chunk.iter().map(|&i| split.train[i].clone()).collect();
            let batch = PairBatcher.batch(items, &device);
            let logits = model.forward(batch.before, batch.after);
            let loss = bce(logits, batch.targets);
            loss_sum += loss.clone().into_scalar().elem::<f64>();
            batches += 1;
            let grads = GradientsParams::from_grads(loss.backward(), &model);
            model = optim.step(1e-3, model, grads);
        }
        if epoch % 10 == 0 {
            println!("  epoch {epoch}: loss {:.4}", loss_sum / batches as f64);
        }
    }

    let valid = model.valid();
    let vdevice: Device<WB> = Default::default();
    let score = |items| scores(&|a, b| valid.forward(a, b), items, &vdevice);
    let train_scored: Vec<(f64, bool)> = score(&split.train)
        .into_iter()
        .zip(split.train.iter().map(|i| i.changed))
        .collect();
    let heldout_scored: Vec<(f64, bool)> = score(&split.heldout)
        .into_iter()
        .zip(split.heldout.iter().map(|i| i.changed))
        .collect();
    let (t, hc, hf) = report("diff-stack", &train_scored, &heldout_scored);
    dump_misses(&heldout_scored, &split.heldout, t);

    std::fs::create_dir_all(&out)?;
    valid.save_file(out.join("effect-model"), &CompactRecorder::new())?;
    let sidecar = GateSidecar {
        format_version: 1,
        pair_w: PAIR_W,
        pair_h: PAIR_H,
        threshold: t,
        epochs,
        train_pairs: split.train.len(),
        heldout_pairs: split.heldout.len(),
        heldout_catch: hc,
        heldout_false_alarm: hf,
    };
    std::fs::write(
        out.join("effect-model.json"),
        serde_json::to_string_pretty(&sidecar)?,
    )?;
    println!("checkpoint -> {}", out.display());
    Ok(())
}
