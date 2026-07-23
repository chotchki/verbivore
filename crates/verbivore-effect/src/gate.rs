//! The runtime effect gate: trained diff-stack checkpoint + its TRAIN-frozen
//! threshold, judging before/after pngs. Phase 4's executor calls this after
//! every write verb (the visual half of the signals-OR-visual gate); the
//! sabotage harness proves it notices when clicks get rewired to dead pixels.

use std::path::Path;

use anyhow::{Context, Result};
use burn::module::Module;
use burn::prelude::*;
use burn::record::CompactRecorder;
use serde::{Deserialize, Serialize};

use crate::models::DiffStackModel;
use crate::pair_data::{PAIR_H, PAIR_W, to_chw};

/// The sidecar json next to the checkpoint — everything a loader needs to run
/// and to refuse checkpoints trained under different assumptions.
#[derive(Debug, Serialize, Deserialize)]
pub struct GateSidecar {
    pub format_version: u32,
    pub pair_w: u32,
    pub pair_h: u32,
    /// Sigmoid-score threshold tuned on TRAIN; >= means Changed.
    pub threshold: f64,
    pub epochs: usize,
    pub train_pairs: usize,
    pub heldout_pairs: usize,
    pub heldout_catch: f64,
    pub heldout_false_alarm: f64,
}

pub struct EffectGate<B: Backend> {
    model: DiffStackModel<B>,
    pub sidecar: GateSidecar,
    device: Device<B>,
}

impl<B: Backend> EffectGate<B> {
    /// Loads `<dir>/effect-model.mpk` + `<dir>/effect-model.json`.
    pub fn load(dir: impl AsRef<Path>, device: &Device<B>) -> Result<Self> {
        let dir = dir.as_ref();
        let sidecar: GateSidecar =
            serde_json::from_str(&std::fs::read_to_string(dir.join("effect-model.json"))?)
                .context("parsing gate sidecar")?;
        anyhow::ensure!(
            sidecar.format_version == 1,
            "unknown gate format_version {}",
            sidecar.format_version
        );
        anyhow::ensure!(
            (sidecar.pair_w, sidecar.pair_h) == (PAIR_W, PAIR_H),
            "checkpoint expects {}x{} inputs, this build uses {}x{}",
            sidecar.pair_w,
            sidecar.pair_h,
            PAIR_W,
            PAIR_H
        );
        let model = DiffStackModel::<B>::init(device)
            .load_file(dir.join("effect-model"), &CompactRecorder::new(), device)
            .context("loading gate checkpoint")?;
        Ok(Self {
            model,
            sidecar,
            device: device.clone(),
        })
    }

    /// Sigmoid change score in [0,1]; higher = changed.
    pub fn score(&self, before_png: &[u8], after_png: &[u8]) -> Result<f64> {
        let (h, w) = (PAIR_H as usize, PAIR_W as usize);
        let tensor = |chw: Vec<f32>| {
            Tensor::<B, 4>::from_data(TensorData::new(chw, [1, 3, h, w]), &self.device)
        };
        let logits = self.model.forward(
            tensor(to_chw(before_png).context("gate before png")?),
            tensor(to_chw(after_png).context("gate after png")?),
        );
        Ok(burn::tensor::activation::sigmoid(logits).into_scalar().elem::<f32>() as f64)
    }

    /// The gate verdict at the frozen threshold.
    pub fn saw_change(&self, before_png: &[u8], after_png: &[u8]) -> Result<(f64, bool)> {
        let score = self.score(before_png, after_png)?;
        Ok((score, score >= self.sidecar.threshold))
    }
}
