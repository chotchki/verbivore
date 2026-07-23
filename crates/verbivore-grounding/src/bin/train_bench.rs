//! Cross-machine training benchmark: identical synthetic dataset (seeded), fixed
//! epochs, per-epoch wall time. Run the SAME command on both machines:
//!
//!   M3 Max:  cargo run -p verbivore-grounding --bin train-bench
//!   2080 Ti: cargo run -p verbivore-grounding --bin train-bench --features cuda
//!
//! Compare secs/epoch. Keep the build profile identical on both sides or the
//! numbers lie. Args: [samples] [epochs] [batch] (defaults 64 3 8).

use image::{DynamicImage, Rgb, RgbImage};
use std::io::Cursor;
use verbivore_dataset::{Bbox, Dataset, ElementLabel};
use verbivore_grounding::data::GroundingDataset;
use verbivore_grounding::train::{TrainConfig, train};

#[cfg(feature = "cuda")]
type Back = burn::backend::Autodiff<burn::backend::Cuda>;
#[cfg(feature = "cuda")]
const BACKEND: &str = "cuda";
#[cfg(not(feature = "cuda"))]
type Back = burn::backend::Autodiff<burn::backend::Wgpu>;
#[cfg(not(feature = "cuda"))]
const BACKEND: &str = "wgpu-metal";

/// Deterministic LCG so both machines train on byte-identical data.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self, bound: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as u32) % bound
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let samples: usize = args.next().map(|a| a.parse()).transpose()?.unwrap_or(64);
    let epochs: usize = args.next().map(|a| a.parse()).transpose()?.unwrap_or(3);
    let batch: usize = args.next().map(|a| a.parse()).transpose()?.unwrap_or(8);

    let dir = tempfile::tempdir()?;
    let ds = Dataset::create(dir.path())?;
    let mut rng = Lcg(42);
    for i in 0..samples {
        let mut img = RgbImage::from_pixel(640, 640, Rgb([245, 245, 245]));
        let mut labels = Vec::new();
        for _ in 0..(2 + rng.next(4)) {
            let (w, h) = (40 + rng.next(160), 20 + rng.next(60));
            let (x, y) = (rng.next(640 - w), rng.next(640 - h));
            for py in y..y + h {
                for px in x..x + w {
                    img.put_pixel(px, py, Rgb([50, 90, 210]));
                }
            }
            labels.push(ElementLabel {
                bbox: Bbox {
                    x: x as f64,
                    y: y as f64,
                    w: w as f64,
                    h: h as f64,
                },
                role: "button".into(),
                name: None,
            });
        }
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(img).write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)?;
        ds.add(&format!("bench://{i}"), 640, 640, 1.0, labels, &bytes)?;
    }

    println!("backend={BACKEND} samples={samples} epochs={epochs} batch={batch}");
    let config = TrainConfig {
        epochs,
        batch_size: batch,
        checkpoint_dir: None,
        ..TrainConfig::default()
    };
    let device = Default::default();
    let outcome = train::<Back>(&config, GroundingDataset::open(dir.path())?, &device)?;

    let per_epoch: Vec<f64> = outcome.history.iter().map(|e| e.seconds).collect();
    // First epoch carries warmup (shader compile / autotune); steady state is the rest.
    let steady: f64 =
        per_epoch.iter().skip(1).sum::<f64>() / per_epoch.len().saturating_sub(1).max(1) as f64;
    println!("secs/epoch={per_epoch:.1?} steady={steady:.2}s");
    Ok(())
}
