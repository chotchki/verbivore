//! Hand-rolled training loop: AdamW over the CenterNet loss with per-epoch
//! checkpoints. Deliberately NOT burn's SupervisedTraining paradigm — the trait
//! plumbing outweighed the TUI it buys, and owning the loop gives the
//! cross-machine benchmark exact control over what a "fixed epoch" measures.

use anyhow::{Context, Result};
use burn::data::dataloader::DataLoaderBuilder;
use burn::module::AutodiffModule;
use burn::optim::{AdamWConfig, GradientsParams, Optimizer};
use burn::prelude::*;
use burn::record::CompactRecorder;
use burn::tensor::backend::AutodiffBackend;
use std::path::PathBuf;
use std::time::Instant;

use crate::data::{GroundingBatcher, GroundingDataset, INPUT_SIZE};
use crate::loss::{build_targets, detection_loss};
use crate::model::GroundingModel;

pub struct TrainConfig {
    pub epochs: usize,
    pub batch_size: usize,
    pub learning_rate: f64,
    pub seed: u64,
    /// Checkpoints land here as model-epoch-<n>; None disables checkpointing.
    pub checkpoint_dir: Option<PathBuf>,
}

impl Default for TrainConfig {
    fn default() -> Self {
        Self {
            epochs: 30,
            batch_size: 8,
            learning_rate: 1e-3,
            seed: 42,
            checkpoint_dir: None,
        }
    }
}

/// Per-epoch numbers, also the raw material for the cross-machine benchmark.
#[derive(Debug, Clone)]
pub struct EpochStats {
    pub epoch: usize,
    pub mean_loss: f64,
    pub seconds: f64,
}

pub struct TrainOutcome<B: AutodiffBackend> {
    pub model: GroundingModel<B>,
    pub history: Vec<EpochStats>,
}

pub fn train<B: AutodiffBackend>(
    config: &TrainConfig,
    dataset: GroundingDataset,
    device: &B::Device,
) -> Result<TrainOutcome<B>> {
    B::seed(device, config.seed);
    let mut model = GroundingModel::<B>::init(device);
    let mut optimizer = AdamWConfig::new().init();

    let loader = DataLoaderBuilder::new(GroundingBatcher)
        .batch_size(config.batch_size)
        .shuffle(config.seed)
        .build(dataset);

    let grid = INPUT_SIZE as usize / crate::model::OUTPUT_STRIDE;
    let mut history = Vec::with_capacity(config.epochs);

    for epoch in 1..=config.epochs {
        let started = Instant::now();
        let mut loss_sum = 0.0f64;
        let mut batches = 0usize;

        for batch in loader.iter() {
            let targets = build_targets::<B>(&batch.boxes, &batch.classes, grid, device);
            let pred = model.forward(batch.images);
            let loss = detection_loss(&pred, &targets);
            loss_sum += loss.clone().into_scalar().elem::<f64>();
            batches += 1;

            let grads = GradientsParams::from_grads(loss.backward(), &model);
            model = optimizer.step(config.learning_rate, model, grads);
        }

        let stats = EpochStats {
            epoch,
            mean_loss: loss_sum / batches.max(1) as f64,
            seconds: started.elapsed().as_secs_f64(),
        };
        println!(
            "epoch {:>3}: loss {:.4} ({:.1}s)",
            stats.epoch, stats.mean_loss, stats.seconds
        );

        if let Some(dir) = &config.checkpoint_dir {
            std::fs::create_dir_all(dir)?;
            model
                .clone()
                .save_file(dir.join(format!("model-epoch-{epoch}")), &CompactRecorder::new())
                .context("saving checkpoint")?;
        }
        history.push(stats);
    }

    Ok(TrainOutcome { model, history })
}

/// Loads a checkpoint for inference on any backend (autodiff not required).
pub fn load_checkpoint<B: Backend>(
    path: impl Into<PathBuf>,
    device: &B::Device,
) -> Result<GroundingModel<B>> {
    use burn::module::Module;
    Ok(GroundingModel::<B>::init(device)
        .load_file(path.into(), &CompactRecorder::new(), device)
        .context("loading checkpoint")?)
}

/// Inference view of a trained autodiff model (drops gradient tracking).
pub fn valid_model<B: AutodiffBackend>(model: &GroundingModel<B>) -> GroundingModel<B::InnerBackend> {
    model.valid()
}
