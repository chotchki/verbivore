//! Effect validation: before/after screenshot pair in, verdict out — meaningful change
//! or ambient noise. Runs after every write verb; must beat an SSIM-threshold baseline
//! to earn its model (see SPEC success criteria). This crate holds that baseline.

pub mod models;
pub mod pair_data;
pub mod train;

use anyhow::{Context, Result};
use verbivore_dataset::{EffectLabel, PairDataset};

/// Downscale width before comparison: kills pixel-level noise (antialiasing,
/// subpixel text) while structural changes survive.
const COMPARE_W: u32 = 320;
/// SSIM window edge; non-overlapping windows, means averaged.
const WINDOW: usize = 8;
const C1: f64 = 0.01 * 0.01;
const C2: f64 = 0.03 * 0.03;

/// Mean structural similarity of two PNGs, grayscale, [0,1]-luma space.
/// 1.0 = identical; UI changes typically land well below 0.99.
pub fn mssim_png(a_png: &[u8], b_png: &[u8]) -> Result<f64> {
    let a = to_gray(a_png).context("decoding first png")?;
    let b = to_gray(b_png).context("decoding second png")?;
    anyhow::ensure!(
        a.dimensions() == b.dimensions(),
        "pair dimensions differ: {:?} vs {:?}",
        a.dimensions(),
        b.dimensions()
    );
    Ok(mssim(&a, &b))
}

fn to_gray(png: &[u8]) -> Result<image::GrayImage> {
    let img = image::load_from_memory(png)?;
    let (w, h) = (img.width(), img.height());
    let scale = COMPARE_W as f64 / w as f64;
    let (nw, nh) = (COMPARE_W, ((h as f64) * scale).round().max(1.0) as u32);
    Ok(image::imageops::resize(
        &img.to_luma8(),
        nw,
        nh,
        image::imageops::FilterType::Triangle,
    ))
}

fn mssim(a: &image::GrayImage, b: &image::GrayImage) -> f64 {
    let (w, h) = (a.width() as usize, a.height() as usize);
    let (mut sum, mut windows) = (0.0f64, 0usize);
    for wy in (0..h.saturating_sub(WINDOW - 1)).step_by(WINDOW) {
        for wx in (0..w.saturating_sub(WINDOW - 1)).step_by(WINDOW) {
            let (mut ma, mut mb) = (0.0f64, 0.0f64);
            for dy in 0..WINDOW {
                for dx in 0..WINDOW {
                    ma += a.get_pixel((wx + dx) as u32, (wy + dy) as u32).0[0] as f64 / 255.0;
                    mb += b.get_pixel((wx + dx) as u32, (wy + dy) as u32).0[0] as f64 / 255.0;
                }
            }
            let n = (WINDOW * WINDOW) as f64;
            ma /= n;
            mb /= n;
            let (mut va, mut vb, mut cov) = (0.0f64, 0.0f64, 0.0f64);
            for dy in 0..WINDOW {
                for dx in 0..WINDOW {
                    let pa = a.get_pixel((wx + dx) as u32, (wy + dy) as u32).0[0] as f64 / 255.0
                        - ma;
                    let pb = b.get_pixel((wx + dx) as u32, (wy + dy) as u32).0[0] as f64 / 255.0
                        - mb;
                    va += pa * pa;
                    vb += pb * pb;
                    cov += pa * pb;
                }
            }
            va /= n - 1.0;
            vb /= n - 1.0;
            cov /= n - 1.0;
            sum += ((2.0 * ma * mb + C1) * (2.0 * cov + C2))
                / ((ma * ma + mb * mb + C1) * (va + vb + C2));
            windows += 1;
        }
    }
    if windows == 0 { 1.0 } else { sum / windows as f64 }
}

/// The tuned baseline: predict Changed when mssim < threshold.
#[derive(Debug, Clone, Copy)]
pub struct BaselineReport {
    pub threshold: f64,
    /// Fraction of true Changed pairs the threshold catches (recall).
    pub catch_rate: f64,
    /// Fraction of true NoChange pairs falsely flagged (FPR).
    pub false_alarm_rate: f64,
    pub accuracy: f64,
    pub pairs: usize,
}

impl BaselineReport {
    /// The SPEC gates the trained model must ALSO clear: >=95% catch, <=5% FA.
    pub fn meets_spec_gates(&self) -> bool {
        self.catch_rate >= 0.95 && self.false_alarm_rate <= 0.05
    }
}

/// Sweeps every candidate threshold (midpoints of observed scores) and keeps
/// the one maximizing Youden's J (catch - false alarm) — accuracy tiebreak.
pub fn tune_ssim_baseline(pairs: &PairDataset) -> Result<BaselineReport> {
    let mut scored: Vec<(f64, bool)> = Vec::new();
    for id in pairs.pair_ids()? {
        let meta = pairs.meta(&id)?;
        let mssim = mssim_png(
            &std::fs::read(pairs.before_path(&id))?,
            &std::fs::read(pairs.after_path(&id))?,
        )
        .with_context(|| format!("pair {id}"))?;
        scored.push((mssim, meta.label == EffectLabel::Changed));
    }
    anyhow::ensure!(!scored.is_empty(), "no pairs to tune against");

    let mut candidates: Vec<f64> = scored.iter().map(|(s, _)| *s).collect();
    candidates.sort_by(f64::total_cmp);
    candidates.dedup();
    let midpoints: Vec<f64> = candidates
        .windows(2)
        .map(|w| (w[0] + w[1]) / 2.0)
        .chain([candidates[0] - 1e-6, candidates[candidates.len() - 1] + 1e-6])
        .collect();

    let total_changed = scored.iter().filter(|(_, c)| *c).count().max(1);
    let total_unchanged = scored.iter().filter(|(_, c)| !*c).count().max(1);
    let mut best: Option<BaselineReport> = None;
    for threshold in midpoints {
        let caught = scored
            .iter()
            .filter(|(s, changed)| *changed && *s < threshold)
            .count();
        let false_alarms = scored
            .iter()
            .filter(|(s, changed)| !*changed && *s < threshold)
            .count();
        let report = BaselineReport {
            threshold,
            catch_rate: caught as f64 / total_changed as f64,
            false_alarm_rate: false_alarms as f64 / total_unchanged as f64,
            accuracy: (caught + (total_unchanged - false_alarms)) as f64 / scored.len() as f64,
            pairs: scored.len(),
        };
        let better = match &best {
            None => true,
            Some(b) => {
                let j = report.catch_rate - report.false_alarm_rate;
                let bj = b.catch_rate - b.false_alarm_rate;
                j > bj || (j == bj && report.accuracy > b.accuracy)
            }
        };
        if better {
            best = Some(report);
        }
    }
    Ok(best.expect("at least one threshold evaluated"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgb, RgbImage};
    use std::io::Cursor;

    fn png(w: u32, h: u32, rect: Option<(u32, u32, u32, u32)>) -> Vec<u8> {
        let mut img = RgbImage::from_pixel(w, h, Rgb([230, 230, 230]));
        if let Some((x0, y0, rw, rh)) = rect {
            for y in y0..(y0 + rh).min(h) {
                for x in x0..(x0 + rw).min(w) {
                    img.put_pixel(x, y, Rgb([20, 40, 180]));
                }
            }
        }
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn identical_images_score_one() {
        let a = png(640, 400, Some((50, 50, 200, 100)));
        assert!((mssim_png(&a, &a).unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn structural_change_scores_below_blank_noise() {
        let blank = png(640, 400, None);
        let panel = png(640, 400, Some((100, 100, 300, 150)));
        let score = mssim_png(&blank, &panel).unwrap();
        assert!(score < 0.95, "a new panel must dent ssim: {score}");
    }
}
