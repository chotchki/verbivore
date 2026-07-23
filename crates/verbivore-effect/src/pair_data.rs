//! Pair loading for the effect models: both pngs decoded, downscaled to a
//! fixed small input, split by PAGE URL — pairs from one page share pixels, so
//! a random split would leak backgrounds between train and eval.

use anyhow::{Context, Result};
use burn::data::dataloader::batcher::Batcher;
use burn::prelude::{Backend, Tensor, TensorData};
use sha2::{Digest, Sha256};
use verbivore_dataset::{EffectLabel, PairDataset};

pub const PAIR_W: u32 = 256;
pub const PAIR_H: u32 = 160;

#[derive(Debug, Clone)]
pub struct PairItem {
    /// CHW RGB [0,1], 3 * PAIR_H * PAIR_W each.
    pub before: Vec<f32>,
    pub after: Vec<f32>,
    pub changed: bool,
}

pub struct PairSplit {
    pub train: Vec<PairItem>,
    pub heldout: Vec<PairItem>,
    /// (mssim, changed) for the heldout slice — the baseline on identical data.
    pub heldout_ssim: Vec<(f64, bool)>,
}

/// Loads the VISIBLE subset (pixel-identical Changed pairs excluded — those are
/// the signal channel's job) and splits ~80/20 by url hash.
pub fn load_visible_split(pairs: &PairDataset) -> Result<PairSplit> {
    let mut split = PairSplit {
        train: Vec::new(),
        heldout: Vec::new(),
        heldout_ssim: Vec::new(),
    };
    for id in pairs.pair_ids()? {
        let meta = pairs.meta(&id)?;
        let before_png = std::fs::read(pairs.before_path(&id))?;
        let after_png = std::fs::read(pairs.after_path(&id))?;
        let changed = meta.label == EffectLabel::Changed;
        if changed && before_png == after_png {
            continue; // invisible effect: unlearnable from pixels
        }
        let item = PairItem {
            before: to_chw(&before_png).with_context(|| format!("pair {id} before"))?,
            after: to_chw(&after_png).with_context(|| format!("pair {id} after"))?,
            changed,
        };
        if url_bucket(&meta.url) < 2 {
            split
                .heldout_ssim
                .push((crate::mssim_png(&before_png, &after_png)?, changed));
            split.heldout.push(item);
        } else {
            split.train.push(item);
        }
    }
    Ok(split)
}

/// Stable url -> 0..10 bucket; <2 = heldout (~20%), independent of insertion order.
fn url_bucket(url: &str) -> u8 {
    Sha256::digest(url.as_bytes())[0] % 10
}

fn to_chw(png: &[u8]) -> Result<Vec<f32>> {
    let img = image::load_from_memory(png)?.to_rgb8();
    let resized = image::imageops::resize(
        &img,
        PAIR_W,
        PAIR_H,
        image::imageops::FilterType::Triangle,
    );
    let plane = (PAIR_W * PAIR_H) as usize;
    let mut buf = vec![0.0f32; 3 * plane];
    for (x, y, p) in resized.enumerate_pixels() {
        let idx = y as usize * PAIR_W as usize + x as usize;
        for c in 0..3 {
            buf[c * plane + idx] = p.0[c] as f32 / 255.0;
        }
    }
    Ok(buf)
}

/// before [B,3,H,W], after [B,3,H,W], targets [B,1] in {0,1}.
#[derive(Debug, Clone)]
pub struct PairBatch<B: Backend> {
    pub before: Tensor<B, 4>,
    pub after: Tensor<B, 4>,
    pub targets: Tensor<B, 2>,
}

#[derive(Clone, Default)]
pub struct PairBatcher;

impl<B: Backend> Batcher<B, PairItem, PairBatch<B>> for PairBatcher {
    fn batch(&self, items: Vec<PairItem>, device: &B::Device) -> PairBatch<B> {
        let n = items.len();
        let (h, w) = (PAIR_H as usize, PAIR_W as usize);
        let mut before = Vec::with_capacity(n * 3 * h * w);
        let mut after = Vec::with_capacity(n * 3 * h * w);
        let mut targets = Vec::with_capacity(n);
        for item in items {
            before.extend_from_slice(&item.before);
            after.extend_from_slice(&item.after);
            targets.push(if item.changed { 1.0f32 } else { 0.0 });
        }
        PairBatch {
            before: Tensor::from_data(TensorData::new(before, [n, 3, h, w]), device),
            after: Tensor::from_data(TensorData::new(after, [n, 3, h, w]), device),
            targets: Tensor::from_data(TensorData::new(targets, [n, 1]), device),
        }
    }
}
