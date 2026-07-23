//! The "can it learn at all" test: overfit two synthetic screenshots. If loss
//! won't collapse on two memorizable images, the model/loss/optimizer wiring is
//! broken somewhere and no amount of real data will save it.

use image::{DynamicImage, Rgb, RgbImage};
use std::io::Cursor;
use verbivore_dataset::{Bbox, Dataset, ElementLabel};
use verbivore_grounding::data::GroundingDataset;
use verbivore_grounding::train::{TrainConfig, train};

type AB = burn::backend::Autodiff<burn::backend::Wgpu>;

fn rect_png(w: u32, h: u32, rect: (u32, u32, u32, u32)) -> Vec<u8> {
    let mut img = RgbImage::from_pixel(w, h, Rgb([240, 240, 240]));
    let (x0, y0, rw, rh) = rect;
    for y in y0..(y0 + rh).min(h) {
        for x in x0..(x0 + rw).min(w) {
            img.put_pixel(x, y, Rgb([30, 60, 200]));
        }
    }
    let mut bytes = Vec::new();
    DynamicImage::ImageRgb8(img)
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

#[test]
#[ignore = "trains ~40 epochs on the gpu (about a minute); run explicitly"]
fn overfits_two_synthetic_screenshots() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let ds = Dataset::create(dir.path())?;
    for (url, rect) in [
        ("synthetic://one", (100u32, 200u32, 200u32, 60u32)),
        ("synthetic://two", (400, 500, 150, 80)),
    ] {
        let (x, y, w, h) = rect;
        ds.add(
            url,
            640,
            640,
            1.0,
            vec![ElementLabel {
                bbox: Bbox {
                    x: x as f64,
                    y: y as f64,
                    w: w as f64,
                    h: h as f64,
                },
                role: "button".into(),
                name: None,
            }],
            &rect_png(640, 640, rect),
        )?;
    }

    let config = TrainConfig {
        epochs: 40,
        batch_size: 2,
        learning_rate: 1e-3,
        checkpoint_dir: Some(dir.path().join("ckpt")),
        ..TrainConfig::default()
    };
    let device = Default::default();
    let outcome = train::<AB>(&config, GroundingDataset::open(dir.path())?, &device)?;

    let first = outcome.history.first().unwrap().mean_loss;
    let last = outcome.history.last().unwrap().mean_loss;
    assert!(
        last < first * 0.2,
        "loss should collapse on two memorizable images: {first:.4} -> {last:.4}"
    );
    assert!(
        dir.path().join("ckpt/model-epoch-40.mpk").exists(),
        "checkpoint written"
    );
    Ok(())
}
