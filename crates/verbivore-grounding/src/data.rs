//! Data pipeline: harvested samples -> letterboxed tensors + detection targets.
//! Aspect is preserved by scale-then-pad, so boxes transform with one linear map.

use anyhow::{Context, Result};
use burn::data::dataloader::batcher::Batcher;
use burn::prelude::{Backend, Tensor, TensorData};
use image::RgbImage;
use std::path::Path;
use verbivore_dataset::{
    AffordanceChannel, AffordanceEvidence, AffordanceSource, Bbox, Dataset as DiskDataset,
    SampleMeta, role_to_class,
};

/// Detector input edge. Everything renders into a square this size.
pub const INPUT_SIZE: u32 = 640;
pub const NUM_CLASSES: usize = verbivore_dataset::INTERACTIVE_ROLES.len();
/// Letterbox padding value, the YOLO-conventional gray (114/255).
const PAD_FILL: f32 = 114.0 / 255.0;

/// How a screenshot maps into the square input: uniform scale, centered pad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Letterbox {
    pub scale: f64,
    pub pad_x: f64,
    pub pad_y: f64,
}

impl Letterbox {
    pub fn fit(w: u32, h: u32) -> Self {
        let scale = (INPUT_SIZE as f64 / w as f64).min(INPUT_SIZE as f64 / h as f64);
        Self {
            scale,
            pad_x: (INPUT_SIZE as f64 - w as f64 * scale) / 2.0,
            pad_y: (INPUT_SIZE as f64 - h as f64 * scale) / 2.0,
        }
    }

    /// Screenshot px -> input px, xyxy.
    pub fn apply(&self, b: Bbox) -> [f32; 4] {
        [
            (b.x * self.scale + self.pad_x) as f32,
            (b.y * self.scale + self.pad_y) as f32,
            ((b.x + b.w) * self.scale + self.pad_x) as f32,
            ((b.y + b.h) * self.scale + self.pad_y) as f32,
        ]
    }
}

/// One training item: image already CHW-normalized in input space.
#[derive(Debug, Clone)]
pub struct GroundingItem {
    /// 3 * INPUT_SIZE * INPUT_SIZE, CHW, [0,1].
    pub image: Vec<f32>,
    /// C.2 affordance prior: 3 * INPUT_SIZE * INPUT_SIZE, CHW, [0,1] —
    /// planes in [`AffordanceChannel`] order (pointer, keyboard, scroll).
    /// All-zero = flat prior = the canvas condition.
    pub prior: Vec<f32>,
    /// xyxy in input px, parallel to `classes`.
    pub boxes: Vec<[f32; 4]>,
    pub classes: Vec<i64>,
    /// xyxy in input px: looks-interactive-but-unlabeled regions. The loss
    /// must not teach these as background.
    pub ignore: Vec<[f32; 4]>,
}

/// Reference "element-sized" area in input px² — the specificity pivot. An
/// evidence rect at or below this area contributes its full source weight;
/// bigger rects dilute as A0/area, so a delegation root or ambient handler
/// spread over the page renders as a faint glow. One rule, no special cases.
const REFERENCE_AREA: f32 = 32.0 * 32.0;

/// How much a source is worth before specificity dilution. Real listener
/// geometry outranks declarative hints; a bare tabindex is the weakest
/// tell. Ambient gets full weight — its viewport-sized area does the
/// diluting.
fn source_weight(source: AffordanceSource) -> f32 {
    match source {
        AffordanceSource::Listener | AffordanceSource::Ambient => 1.0,
        AffordanceSource::NativeTag => 0.9,
        AffordanceSource::CursorPointer => 0.8,
        AffordanceSource::Scrollable => 0.7,
        AffordanceSource::Tabindex => 0.5,
    }
}

/// Rasterizes vector evidence into the 3 prior planes: additive heat,
/// heat = source weight * min(1, A0/area), clamped to 1.0 per cell.
/// Pub because it IS the runtime rasterizer too (C.6) — harvest and
/// inference share one code path by construction.
pub fn rasterize_prior(evidence: &[AffordanceEvidence], lb: &Letterbox) -> Vec<f32> {
    let side = INPUT_SIZE as usize;
    let plane = side * side;
    let mut prior = vec![0.0f32; 3 * plane];
    for e in evidence {
        let [x0, y0, x1, y1] = lb.apply(e.bbox);
        let (x0, y0) = (x0.max(0.0) as usize, y0.max(0.0) as usize);
        let (x1, y1) = (
            (x1.min(side as f32) as usize).max(x0),
            (y1.min(side as f32) as usize).max(y0),
        );
        let area = ((x1 - x0) * (y1 - y0)) as f32;
        if area < 1.0 {
            continue;
        }
        let heat = source_weight(e.source) * (REFERENCE_AREA / area).min(1.0);
        let ch = match e.channel {
            AffordanceChannel::Pointer => 0,
            AffordanceChannel::Keyboard => 1,
            AffordanceChannel::Scroll => 2,
        };
        for y in y0..y1 {
            let row = ch * plane + y * side;
            for x in x0..x1 {
                prior[row + x] = (prior[row + x] + heat).min(1.0);
            }
        }
    }
    prior
}

/// Burn-facing view of a harvested dataset directory.
pub struct GroundingDataset {
    disk: DiskDataset,
    ids: Vec<String>,
    cache: Option<Vec<std::sync::OnceLock<GroundingItem>>>,
}

impl GroundingDataset {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let disk = DiskDataset::open(root.as_ref().to_path_buf())?;
        let ids = disk.sample_ids()?;
        Ok(Self {
            disk,
            ids,
            cache: None,
        })
    }

    /// Caches decoded items in memory after first touch: png decode dominates
    /// epoch time otherwise (5x on real screenshots). Costs ~9.8MB per sample
    /// resident (6 * 640 * 640 f32 — rgb + prior planes) — right for training
    /// on a big-RAM box, wrong for streaming; pick per call site.
    pub fn open_cached(root: impl AsRef<Path>) -> Result<Self> {
        let mut ds = Self::open(root)?;
        ds.cache = Some((0..ds.ids.len()).map(|_| std::sync::OnceLock::new()).collect());
        Ok(ds)
    }

    fn load(&self, id: &str) -> Result<GroundingItem> {
        let meta = self.disk.meta(id)?;
        let png = std::fs::read(self.disk.png_path(id))?;
        let img = image::load_from_memory(&png)
            .with_context(|| format!("decoding sample {id}"))?
            .to_rgb8();
        Ok(item_from_parts(&img, &meta))
    }
}

fn item_from_parts(img: &RgbImage, meta: &SampleMeta) -> GroundingItem {
    let (w, h) = img.dimensions();
    let lb = Letterbox::fit(w, h);
    let scaled_w = ((w as f64) * lb.scale).round() as u32;
    let scaled_h = ((h as f64) * lb.scale).round() as u32;
    let resized = image::imageops::resize(
        img,
        scaled_w.max(1),
        scaled_h.max(1),
        image::imageops::FilterType::Triangle,
    );

    let side = INPUT_SIZE as usize;
    let plane = side * side;
    let mut image_buf = vec![PAD_FILL; 3 * plane];
    let off_x = lb.pad_x.round() as usize;
    let off_y = lb.pad_y.round() as usize;
    for (x, y, pixel) in resized.enumerate_pixels() {
        let ix = x as usize + off_x;
        let iy = y as usize + off_y;
        if ix < side && iy < side {
            for c in 0..3 {
                image_buf[c * plane + iy * side + ix] = pixel.0[c] as f32 / 255.0;
            }
        }
    }

    let mut boxes = Vec::new();
    let mut classes = Vec::new();
    for label in &meta.labels {
        // Unknown roles can only exist if the dataset outgrew this build's class
        // list; skipping them beats guessing a class.
        if let Some(class) = role_to_class(&label.role) {
            boxes.push(lb.apply(label.bbox));
            classes.push(class as i64);
        }
    }
    GroundingItem {
        image: image_buf,
        prior: rasterize_prior(&meta.affordance, &lb),
        boxes,
        classes,
        ignore: meta.ignore.iter().map(|b| lb.apply(*b)).collect(),
    }
}

impl burn::data::dataset::Dataset<GroundingItem> for GroundingDataset {
    fn get(&self, index: usize) -> Option<GroundingItem> {
        let id = self.ids.get(index)?;
        // A corrupt sample panics with its id: training on silently-skipped data
        // is worse than crashing.
        let load = || {
            self.load(id)
                .unwrap_or_else(|e| panic!("loading sample {id}: {e:#}"))
        };
        Some(match &self.cache {
            Some(slots) => slots[index].get_or_init(load).clone(),
            None => load(),
        })
    }

    fn len(&self) -> usize {
        self.ids.len()
    }
}

/// Images stack; boxes/classes/ignore stay ragged per item — the loss walks them.
#[derive(Debug, Clone)]
pub struct GroundingBatch<B: Backend> {
    pub images: Tensor<B, 4>,
    pub boxes: Vec<Vec<[f32; 4]>>,
    pub classes: Vec<Vec<i64>>,
    pub ignore: Vec<Vec<[f32; 4]>>,
}

#[derive(Clone, Default)]
pub struct GroundingBatcher;

impl<B: Backend> Batcher<B, GroundingItem, GroundingBatch<B>> for GroundingBatcher {
    fn batch(&self, items: Vec<GroundingItem>, device: &B::Device) -> GroundingBatch<B> {
        let n = items.len();
        let side = INPUT_SIZE as usize;
        // 6 channels: rgb then the 3 affordance planes. Fusing here keeps
        // the model's forward signature (and every call site) unchanged.
        let mut flat = Vec::with_capacity(n * 6 * side * side);
        let mut boxes = Vec::with_capacity(n);
        let mut classes = Vec::with_capacity(n);
        let mut ignore = Vec::with_capacity(n);
        for item in items {
            flat.extend_from_slice(&item.image);
            flat.extend_from_slice(&item.prior);
            boxes.push(item.boxes);
            classes.push(item.classes);
            ignore.push(item.ignore);
        }
        let images = Tensor::from_data(TensorData::new(flat, [n, 6, side, side]), device);
        GroundingBatch {
            images,
            boxes,
            classes,
            ignore,
        }
    }
}
