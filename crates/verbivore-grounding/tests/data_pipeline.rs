use image::{DynamicImage, RgbImage};
use std::io::Cursor;
use verbivore_dataset::{Bbox, Dataset, ElementLabel};
use verbivore_grounding::data::{
    GroundingBatch, GroundingBatcher, GroundingDataset, INPUT_SIZE, Letterbox,
};

type B = burn::backend::NdArray<f32>;

fn label(role: &str, x: f64, y: f64, w: f64, h: f64) -> ElementLabel {
    ElementLabel {
        bbox: Bbox { x, y, w, h },
        role: role.into(),
        name: None,
    }
}

fn png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    let img = RgbImage::from_pixel(w, h, image::Rgb(rgb));
    let mut bytes = Vec::new();
    DynamicImage::ImageRgb8(img)
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

#[test]
fn letterbox_scales_then_pads_centered() {
    // 200x100 -> scale 3.2 (640/200), scaled 640x320, vertical pad 160.
    let lb = Letterbox::fit(200, 100);
    assert!((lb.scale - 3.2).abs() < 1e-9);
    assert!((lb.pad_x - 0.0).abs() < 1e-9);
    assert!((lb.pad_y - 160.0).abs() < 1e-9);

    let out = lb.apply(Bbox {
        x: 50.0,
        y: 25.0,
        w: 100.0,
        h: 50.0,
    });
    assert_eq!(out, [160.0, 240.0, 480.0, 400.0]);
}

#[test]
fn loads_letterboxed_items_with_class_indices() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let ds = Dataset::create(dir.path())?;
    ds.add(
        "http://fixture/",
        200,
        100,
        1.0,
        vec![
            label("button", 50.0, 25.0, 100.0, 50.0),
            // A role outside the class list must be skipped, not guessed.
            label("figure", 0.0, 0.0, 10.0, 10.0),
        ],
        &png(200, 100, [255, 0, 0]),
    )?;

    let gd = GroundingDataset::open(dir.path())?;
    use burn::data::dataset::Dataset as BurnDataset;
    assert_eq!(BurnDataset::len(&gd), 1);
    let item = BurnDataset::get(&gd, 0).unwrap();

    let side = INPUT_SIZE as usize;
    assert_eq!(item.image.len(), 3 * side * side);
    assert_eq!(item.boxes.len(), 1, "unknown role should be dropped");
    assert_eq!(item.classes, vec![0], "button is class 0");
    assert_eq!(item.boxes[0], [160.0, 240.0, 480.0, 400.0]);

    // Top-left corner is letterbox padding (gray), image center is the red fill.
    let pad_px = item.image[0];
    assert!((pad_px - 114.0 / 255.0).abs() < 0.02, "pad was {pad_px}");
    let center = (side / 2) * side + side / 2;
    assert!((item.image[center] - 1.0).abs() < 0.02, "red channel center");
    assert!(item.image[side * side + center] < 0.02, "green channel center");
    Ok(())
}

#[test]
fn batcher_stacks_images_and_keeps_targets_ragged() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let ds = Dataset::create(dir.path())?;
    ds.add(
        "http://a/",
        200,
        100,
        1.0,
        vec![label("button", 0.0, 0.0, 20.0, 20.0)],
        &png(200, 100, [10, 20, 30]),
    )?;
    ds.add(
        "http://b/",
        100,
        200,
        1.0,
        vec![
            label("link", 0.0, 0.0, 20.0, 20.0),
            label("tab", 5.0, 5.0, 20.0, 20.0),
        ],
        &png(100, 200, [40, 50, 60]),
    )?;

    let gd = GroundingDataset::open(dir.path())?;
    use burn::data::dataloader::batcher::Batcher;
    use burn::data::dataset::Dataset as BurnDataset;
    let items = vec![
        BurnDataset::get(&gd, 0).unwrap(),
        BurnDataset::get(&gd, 1).unwrap(),
    ];
    let batch: GroundingBatch<B> = GroundingBatcher.batch(items, &Default::default());

    assert_eq!(
        batch.images.dims(),
        [2, 3, INPUT_SIZE as usize, INPUT_SIZE as usize]
    );
    let counts: Vec<usize> = batch.classes.iter().map(Vec::len).collect();
    assert_eq!(counts.iter().sum::<usize>(), 3);
    assert_eq!(batch.boxes.iter().map(Vec::len).collect::<Vec<_>>(), counts);
    Ok(())
}
