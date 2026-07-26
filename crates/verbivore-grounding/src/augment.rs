//! C.3 prior dropout: per-channel stochastic degradation of the affordance
//! planes, applied at TRAIN time only. This is the anti-shortcut mechanism —
//! a model that leans on a clean prior collapses on canvas, where the prior
//! is flat — and the same knob buys robustness to the runtime rasterizer's
//! cheaper approximation. Deterministic given the rng state (B.1 proved the
//! whole loop bit-reproducible; augmentation must not break that).

use burn::prelude::*;
use burn::tensor::module::{avg_pool2d, max_pool2d};

/// Splitmix-style step; good enough spread for augmentation draws.
pub fn lcg_next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state
}

fn unit(state: &mut u64) -> f32 {
    (lcg_next(state) >> 40) as f32 / (1u32 << 24) as f32
}

/// Degrades the 3 prior channels of a fused [n, 6, h, w] batch; rgb passes
/// through untouched. 15% of batches go ALL-FLAT (the canvas condition,
/// trained directly — independent per-channel drops would make it a 0.8%
/// rarity); otherwise each channel independently drops, attenuates,
/// flattens to its mean, blurs or dilates.
pub fn degrade_prior<B: Backend>(images: Tensor<B, 4>, rng: &mut u64) -> Tensor<B, 4> {
    let [n, c, h, w] = images.dims();
    debug_assert_eq!(c, 6, "degrade_prior expects rgb + 3 prior planes");
    let rgb = images.clone().slice([0..n, 0..3, 0..h, 0..w]);

    if unit(rng) < 0.15 {
        let flat = Tensor::zeros([n, 3, h, w], &images.device());
        return Tensor::cat(vec![rgb, flat], 1);
    }

    let mut planes = Vec::with_capacity(3);
    for ch in 3..6 {
        let p = images.clone().slice([0..n, ch..ch + 1, 0..h, 0..w]);
        let r = unit(rng);
        let p = if r < 0.20 {
            // Drop: this channel's evidence simply isn't available.
            Tensor::zeros_like(&p)
        } else if r < 0.35 {
            // Attenuate: weaker evidence than harvest saw.
            p * (0.2 + 0.6 * unit(rng))
        } else if r < 0.45 {
            // Flatten: keep the page-level amount, destroy localization.
            let m = p.clone().mean_dim(2).mean_dim(3);
            Tensor::ones_like(&p) * m
        } else if r < 0.55 {
            // Blur: sloppy localization (runtime approximation error).
            let k = [5usize, 9, 17][(lcg_next(rng) % 3) as usize];
            avg_pool2d(p, [k, k], [1, 1], [k / 2, k / 2], false, false)
        } else if r < 0.65 {
            // Dilate: over-eager rects (max-filter smears heat outward).
            let k = [5usize, 9][(lcg_next(rng) % 2) as usize];
            max_pool2d(p, [k, k], [1, 1], [k / 2, k / 2], [1, 1], false)
        } else {
            p
        };
        planes.push(p);
    }
    let mut all = vec![rgb];
    all.extend(planes);
    Tensor::cat(all, 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    type B = burn::backend::NdArray<f32>;

    fn fused(n: usize, side: usize) -> Tensor<B, 4> {
        // rgb = 0.5 everywhere, priors = 1.0 everywhere: any degradation is
        // visible as a sub-1.0 prior cell, and rgb corruption as != 0.5.
        let rgb = Tensor::full([n, 3, side, side], 0.5, &Default::default());
        let prior = Tensor::ones([n, 3, side, side], &Default::default());
        Tensor::cat(vec![rgb, prior], 1)
    }

    #[test]
    fn rgb_never_changes_and_draws_are_deterministic() {
        let mut rng_a = 7u64;
        let mut rng_b = 7u64;
        for _ in 0..20 {
            let out_a = degrade_prior(fused(2, 32), &mut rng_a);
            let out_b = degrade_prior(fused(2, 32), &mut rng_b);
            assert_eq!(out_a.dims(), [2, 6, 32, 32]);
            let rgb: Vec<f32> = out_a
                .clone()
                .slice([0..2, 0..3, 0..32, 0..32])
                .into_data()
                .to_vec()
                .unwrap();
            assert!(rgb.iter().all(|&v| (v - 0.5).abs() < 1e-6), "rgb touched");
            assert_eq!(
                out_a.into_data().to_vec::<f32>().unwrap(),
                out_b.into_data().to_vec::<f32>().unwrap(),
                "same rng state must give the same degradation"
            );
        }
    }

    #[test]
    fn all_flat_and_identity_both_occur() {
        let mut rng = 1u64;
        let (mut saw_flat, mut saw_identity) = (false, false);
        for _ in 0..80 {
            let prior: Vec<f32> = degrade_prior(fused(1, 16), &mut rng)
                .slice([0..1, 3..6, 0..16, 0..16])
                .into_data()
                .to_vec()
                .unwrap();
            if prior.iter().all(|&v| v == 0.0) {
                saw_flat = true;
            }
            if prior.iter().all(|&v| (v - 1.0).abs() < 1e-6) {
                saw_identity = true;
            }
        }
        assert!(saw_flat, "the canvas condition must be trained");
        assert!(saw_identity, "clean priors must survive sometimes");
    }
}
