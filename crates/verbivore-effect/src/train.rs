//! Shared scoring + threshold machinery for the effect models. The 3.5 spike
//! and the 3.6 trainer both go through here so their numbers stay comparable;
//! the threshold-TRANSFER half (tune on train, freeze for heldout) lives in
//! `best_operating_point` + `operating_point_at`.

use burn::data::dataloader::batcher::Batcher;
use burn::prelude::*;

use crate::pair_data::{PairBatcher, PairItem};

/// Tiny deterministic LCG — reproducible epoch shuffles without a rand dep.
pub struct Lcg(pub u64);

impl Lcg {
    pub fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % bound
    }

    pub fn shuffle(&mut self, order: &mut [usize]) {
        for i in (1..order.len()).rev() {
            order.swap(i, self.next(i + 1));
        }
    }
}

/// Stable BCE-with-logits, mean over batch.
pub fn bce<B: Backend>(logits: Tensor<B, 2>, targets: Tensor<B, 2>) -> Tensor<B, 1> {
    let neg_abs = logits.clone().abs().neg();
    (logits.clone().clamp_min(0.0) - logits * targets + (neg_abs.exp() + 1.0).log()).mean()
}

/// Sigmoid scores for every item, batched; higher = changed.
pub fn scores<B: Backend, M>(model: &M, items: &[PairItem], device: &Device<B>) -> Vec<f64>
where
    M: Fn(Tensor<B, 4>, Tensor<B, 4>) -> Tensor<B, 2>,
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

/// (threshold, catch, false_alarm) maximizing Youden's J. Candidates are
/// MIDPOINTS between adjacent observed scores (plus the ends): identical
/// catch/FA on the tuning set as sweeping the scores themselves, but the
/// frozen threshold sits off the knife edge when it transfers to new data.
pub fn best_operating_point(scored: &[(f64, bool)]) -> (f64, f64, f64) {
    let mut candidates: Vec<f64> = scored.iter().map(|(s, _)| *s).collect();
    candidates.sort_by(f64::total_cmp);
    candidates.dedup();
    let midpoints: Vec<f64> = candidates
        .windows(2)
        .map(|w| (w[0] + w[1]) / 2.0)
        .chain([
            candidates[0] - 1e-6,
            candidates[candidates.len() - 1] + 1e-6,
        ])
        .collect();
    let mut best = (0.0, 0.0, 1.0);
    for t in midpoints {
        let (catch, fa) = operating_point_at(scored, t);
        if catch - fa > best.1 - best.2 {
            best = (t, catch, fa);
        }
    }
    best
}

/// (catch, false_alarm) at a FROZEN threshold — the transfer half of the
/// protocol. Score >= t predicts Changed.
pub fn operating_point_at(scored: &[(f64, bool)], t: f64) -> (f64, f64) {
    let changed = scored.iter().filter(|(_, c)| *c).count().max(1) as f64;
    let unchanged = scored.iter().filter(|(_, c)| !*c).count().max(1) as f64;
    let catch = scored.iter().filter(|(s, c)| *c && *s >= t).count() as f64 / changed;
    let fa = scored.iter().filter(|(s, c)| !*c && *s >= t).count() as f64 / unchanged;
    (catch, fa)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tuned_threshold_separates_and_transfers() {
        let train = [(0.1, false), (0.2, false), (0.8, true), (0.9, true)];
        let (t, catch, fa) = best_operating_point(&train);
        assert!((0.2..=0.8).contains(&t), "midpoint threshold expected: {t}");
        assert_eq!((catch, fa), (1.0, 0.0));
        // Frozen threshold on a shifted slice: one changed pair slips under.
        let heldout = [(0.15, false), (0.4, true), (0.85, true)];
        let (hc, hf) = operating_point_at(&heldout, t);
        assert_eq!(hf, 0.0);
        assert!(hc < 1.0, "the 0.4 changed pair must be missed at t={t}");
    }
}
