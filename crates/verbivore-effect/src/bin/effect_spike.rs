//! The 3.5 spike: siamese vs diff-stack on the visible pair corpus, three-way
//! compared against SSIM on the SAME url-held-out slice. All three get their
//! best (oracle) threshold on the heldout scores — symmetric and fair for a
//! spike; proper train-tuned thresholds come with 3.6.
//!
//!   cargo run --release -p verbivore-effect --bin effect-spike -- <pairs_dir> [epochs]

use burn::backend::Autodiff;
use burn::data::dataloader::batcher::Batcher;
use burn::module::AutodiffModule;
use burn::optim::{AdamWConfig, GradientsParams, Optimizer};
use burn::prelude::*;
use verbivore_dataset::PairDataset;
use verbivore_effect::models::{DiffStackModel, SiameseModel};
use verbivore_effect::pair_data::{PairBatcher, PairItem, load_visible_split};

type WB = burn::backend::Wgpu;
type AB = Autodiff<WB>;

struct Lcg(u64);
impl Lcg {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % bound
    }
}

/// Stable BCE-with-logits, mean over batch.
fn bce<B: Backend>(logits: Tensor<B, 2>, targets: Tensor<B, 2>) -> Tensor<B, 1> {
    let neg_abs = logits.clone().abs().neg();
    (logits.clone().clamp_min(0.0) - logits * targets + (neg_abs.exp() + 1.0).log()).mean()
}

fn scores<M>(model: &M, items: &[PairItem], device: &Device<WB>) -> Vec<f64>
where
    M: Fn(Tensor<WB, 4>, Tensor<WB, 4>) -> Tensor<WB, 2>,
{
    let mut out = Vec::with_capacity(items.len());
    for chunk in items.chunks(16) {
        let batch = PairBatcher.batch(chunk.to_vec(), device);
        let logits = model(batch.before, batch.after);
        let probs = burn::tensor::activation::sigmoid(logits);
        out.extend(
            probs
                .to_data()
                .to_vec::<f32>()
                .unwrap()
                .into_iter()
                .map(|v| v as f64),
        );
    }
    out
}

/// Oracle threshold sweep: best Youden's J on (score, changed); higher = changed.
fn best_operating_point(scored: &[(f64, bool)]) -> (f64, f64, f64) {
    let changed = scored.iter().filter(|(_, c)| *c).count().max(1) as f64;
    let unchanged = scored.iter().filter(|(_, c)| !*c).count().max(1) as f64;
    let mut best = (0.0, 0.0, 1.0);
    let mut thresholds: Vec<f64> = scored.iter().map(|(s, _)| *s).collect();
    thresholds.sort_by(f64::total_cmp);
    for t in thresholds {
        let catch = scored.iter().filter(|(s, c)| *c && *s >= t).count() as f64 / changed;
        let fa = scored.iter().filter(|(s, c)| !*c && *s >= t).count() as f64 / unchanged;
        if catch - fa > best.1 - best.2 {
            best = (t, catch, fa);
        }
    }
    best
}

fn train_and_eval<Init, Fwd, TrainFwd, M, TM>(
    name: &str,
    init: Init,
    fwd_valid: Fwd,
    fwd_train: TrainFwd,
    train: &[PairItem],
    heldout: &[PairItem],
    epochs: usize,
) where
    Init: Fn(&Device<AB>) -> TM,
    TM: AutodiffModule<AB, InnerModule = M> + core::fmt::Debug,
    Fwd: Fn(&M, Tensor<WB, 4>, Tensor<WB, 4>) -> Tensor<WB, 2>,
    TrainFwd: Fn(&TM, Tensor<AB, 4>, Tensor<AB, 4>) -> Tensor<AB, 2>,
    TM: Clone,
{
    let device: Device<AB> = Default::default();
    let mut model = init(&device);
    let mut optim = AdamWConfig::new().init();
    let mut rng = Lcg(7);

    for epoch in 1..=epochs {
        let mut order: Vec<usize> = (0..train.len()).collect();
        for i in (1..order.len()).rev() {
            order.swap(i, rng.next(i + 1));
        }
        let mut loss_sum = 0.0;
        let mut batches = 0;
        for chunk in order.chunks(16) {
            let items: Vec<PairItem> = chunk.iter().map(|&i| train[i].clone()).collect();
            let batch = PairBatcher.batch(items, &device);
            let logits = fwd_train(&model, batch.before, batch.after);
            let loss = bce(logits, batch.targets);
            loss_sum += loss.clone().into_scalar().elem::<f64>();
            batches += 1;
            let grads = GradientsParams::from_grads(loss.backward(), &model);
            model = optim.step(1e-3, model, grads);
        }
        if epoch % 10 == 0 {
            println!("  {name} epoch {epoch}: loss {:.4}", loss_sum / batches as f64);
        }
    }

    let valid = model.valid();
    let vdevice: Device<WB> = Default::default();
    let s = scores(&|a, b| fwd_valid(&valid, a, b), heldout, &vdevice);
    let scored: Vec<(f64, bool)> = s
        .into_iter()
        .zip(heldout.iter().map(|i| i.changed))
        .collect();
    let (t, catch, fa) = best_operating_point(&scored);
    println!("{name}: heldout catch={catch:.3} false-alarm={fa:.3} (threshold {t:.3})");
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("pairs dataset dir");
    let epochs: usize = args.next().map(|a| a.parse()).transpose()?.unwrap_or(40);

    let split = load_visible_split(&PairDataset::open(dir)?)?;
    println!(
        "visible pairs: train={} heldout={} (split by url)",
        split.train.len(),
        split.heldout.len()
    );

    // SSIM on the identical heldout slice, same oracle threshold treatment.
    // Score = 1 - mssim so "higher = changed" matches the models.
    let ssim_scored: Vec<(f64, bool)> = split
        .heldout_ssim
        .iter()
        .map(|(s, c)| (1.0 - s, *c))
        .collect();
    let (t, catch, fa) = best_operating_point(&ssim_scored);
    println!("ssim-baseline: heldout catch={catch:.3} false-alarm={fa:.3} (threshold {t:.4})");

    train_and_eval(
        "siamese",
        SiameseModel::<AB>::init,
        |m: &SiameseModel<WB>, a, b| m.forward(a, b),
        |m: &SiameseModel<AB>, a, b| m.forward(a, b),
        &split.train,
        &split.heldout,
        epochs,
    );
    train_and_eval(
        "diff-stack",
        DiffStackModel::<AB>::init,
        |m: &DiffStackModel<WB>, a, b| m.forward(a, b),
        |m: &DiffStackModel<AB>, a, b| m.forward(a, b),
        &split.train,
        &split.heldout,
        epochs,
    );
    Ok(())
}
