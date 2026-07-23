//! On-disk dataset format: written by the harvester, read by training. Portable by
//! design — harvest on one machine, train on another (the files ARE the interface).
//! This crate must stay browser-free: training links it, and training should never
//! pull chromiumoxide.
//!
//! Layout: `<root>/dataset.json` (format manifest) + `<root>/samples/<id>.png` and
//! `<id>.json` sidecars. Ids are content hashes of the png, so identical
//! screenshots dedupe to one sample no matter how often they're captured.

pub mod rank;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const FORMAT_VERSION: u32 = 1;

/// The canonical interactive-role list: what the harvester labels AND the class
/// set the detector trains on. One list, or the two silently drift. Order is
/// the class index — append only, reordering invalidates every trained model.
pub const INTERACTIVE_ROLES: &[&str] = &[
    "button",
    "link",
    "textbox",
    "searchbox",
    "checkbox",
    "radio",
    "combobox",
    "listbox",
    "option",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "tab",
    "switch",
    "slider",
    "spinbutton",
];

/// Class index for a role, None for roles the detector doesn't know.
pub fn role_to_class(role: &str) -> Option<usize> {
    INTERACTIVE_ROLES.iter().position(|r| *r == role)
}

/// Bounding box in SCREENSHOT pixels (CSS px already scaled by the sample's
/// dpr); break that invariant and every label is silently misaligned.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bbox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Intersection-over-union; 0.0 for degenerate boxes.
pub fn iou(a: &Bbox, b: &Bbox) -> f64 {
    let ix = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let iy = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    if ix <= 0.0 || iy <= 0.0 {
        return 0.0;
    }
    let inter = ix * iy;
    let union = a.w * a.h + b.w * b.h - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

/// Snaps a predicted box (vision output) to the best-overlapping label —
/// the bridge from pixels back to the a11y element a selector can address.
/// None when nothing clears `min_iou`: a detection over dead space must not
/// snap to whatever is least far away.
pub fn snap_to_label<'l>(
    pred: &Bbox,
    labels: &'l [ElementLabel],
    min_iou: f64,
) -> Option<&'l ElementLabel> {
    labels
        .iter()
        .map(|l| (l, iou(pred, &l.bbox)))
        .filter(|(_, v)| *v >= min_iou)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(l, _)| l)
}

/// One training label: where an interactive element is and what it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ElementLabel {
    pub bbox: Bbox,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Sidecar metadata for one screenshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleMeta {
    pub id: String,
    pub url: String,
    /// CSS px; the png's pixel dimensions are viewport * dpr.
    pub viewport_w: i64,
    pub viewport_h: i64,
    pub dpr: f64,
    pub captured_at_unix: u64,
    pub labels: Vec<ElementLabel>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    format_version: u32,
}

/// CDP-derived activity counts around an action. Training labels only — verb
/// runtime never sees these (canvas pages can't produce them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EffectSignals {
    pub dom_mutations: u64,
    pub aria_mutations: u64,
    pub network_requests: u64,
    /// The action replaced the document entirely (link clicks do this). The
    /// strongest possible Changed signal — counters don't survive it, so it's
    /// detected by their absence.
    #[serde(default)]
    pub navigated: bool,
}

/// What the pair teaches: did the action meaningfully change the page?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectLabel {
    Changed,
    NoChange,
}

/// v1 rule: any signal delta above the ambient-subtracted floor is a change.
/// Known noise: heavily animated pages can leak ambient activity into the
/// action window — the subtraction narrows it, doesn't erase it.
pub fn label_from_signals(action_delta: &EffectSignals) -> EffectLabel {
    if action_delta.navigated
        || action_delta.dom_mutations > 0
        || action_delta.aria_mutations > 0
        || action_delta.network_requests > 0
    {
        EffectLabel::Changed
    } else {
        EffectLabel::NoChange
    }
}

/// Sidecar for one before/after pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairMeta {
    pub id: String,
    pub url: String,
    /// None = a no-action control pair: time passed, nobody clicked. Those are
    /// NoChange by construction — ambient animation must read as noise.
    pub click: Option<(f64, f64)>,
    pub viewport_w: i64,
    pub viewport_h: i64,
    pub dpr: f64,
    pub settle_ms: u64,
    /// Ambient-subtracted action-window activity.
    pub signals: EffectSignals,
    /// The page's no-action noise floor over an equal window.
    pub ambient: EffectSignals,
    pub label: EffectLabel,
}

/// Effect-pair storage: `<root>/pairs-dataset.json` + `<root>/pairs/<id>.before.png`,
/// `<id>.after.png`, `<id>.json`. Content-addressed over both images + click, so
/// re-captures dedupe but the same screen clicked at two spots stays two pairs.
pub struct PairDataset {
    root: PathBuf,
}

impl PairDataset {
    pub fn create(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("pairs"))?;
        let manifest = root.join("pairs-dataset.json");
        if manifest.exists() {
            return Self::open(root);
        }
        fs::write(
            &manifest,
            serde_json::to_vec_pretty(&Manifest {
                format_version: FORMAT_VERSION,
            })?,
        )?;
        Ok(Self { root })
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let manifest: Manifest = serde_json::from_slice(
            &fs::read(root.join("pairs-dataset.json"))
                .context("not a pair dataset: no pairs-dataset.json")?,
        )?;
        if manifest.format_version != FORMAT_VERSION {
            bail!(
                "pair dataset format v{} but this build reads v{FORMAT_VERSION}",
                manifest.format_version
            );
        }
        Ok(Self { root })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &self,
        url: &str,
        click: Option<(f64, f64)>,
        viewport_w: i64,
        viewport_h: i64,
        dpr: f64,
        settle_ms: u64,
        signals: EffectSignals,
        ambient: EffectSignals,
        before_png: &[u8],
        after_png: &[u8],
    ) -> Result<AddOutcome> {
        let mut hasher = Sha256::new();
        hasher.update(before_png);
        hasher.update(after_png);
        match click {
            Some((x, y)) => hasher.update(format!("{x:.1},{y:.1}")),
            None => hasher.update("noaction"),
        }
        let id = hex16(&hasher.finalize());
        let meta_path = self.meta_json_path(&id);
        if meta_path.exists() {
            return Ok(AddOutcome { id, deduped: true });
        }
        fs::write(self.before_path(&id), before_png)?;
        fs::write(self.after_path(&id), after_png)?;
        let meta = PairMeta {
            id: id.clone(),
            url: url.to_owned(),
            click,
            viewport_w,
            viewport_h,
            dpr,
            settle_ms,
            signals,
            ambient,
            label: match click {
                Some(_) => label_from_signals(&signals),
                None => EffectLabel::NoChange,
            },
        };
        fs::write(meta_path, serde_json::to_vec_pretty(&meta)?)?;
        Ok(AddOutcome { id, deduped: false })
    }

    pub fn pair_ids(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        for entry in fs::read_dir(self.root.join("pairs"))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                ids.push(stem.to_owned());
            }
        }
        ids.sort();
        Ok(ids)
    }

    pub fn meta(&self, id: &str) -> Result<PairMeta> {
        Ok(serde_json::from_slice(&fs::read(self.meta_json_path(id))?)?)
    }

    pub fn before_path(&self, id: &str) -> PathBuf {
        self.root.join("pairs").join(format!("{id}.before.png"))
    }

    pub fn after_path(&self, id: &str) -> PathBuf {
        self.root.join("pairs").join(format!("{id}.after.png"))
    }

    fn meta_json_path(&self, id: &str) -> PathBuf {
        self.root.join("pairs").join(format!("{id}.json"))
    }
}

pub struct Dataset {
    root: PathBuf,
}

#[derive(Debug)]
pub struct AddOutcome {
    pub id: String,
    /// True when this exact screenshot was already in the dataset. First capture
    /// wins — a dupe's (possibly different) url/labels are dropped, not merged.
    pub deduped: bool,
}

impl Dataset {
    /// Creates the layout, or opens it if already present (idempotent).
    pub fn create(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("samples"))?;
        let manifest = root.join("dataset.json");
        if manifest.exists() {
            return Self::open(root);
        }
        fs::write(
            &manifest,
            serde_json::to_vec_pretty(&Manifest {
                format_version: FORMAT_VERSION,
            })?,
        )?;
        Ok(Self { root })
    }

    /// Opens an existing dataset; refuses a missing or mismatched manifest.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let manifest: Manifest = serde_json::from_slice(
            &fs::read(root.join("dataset.json")).context("not a dataset: no dataset.json")?,
        )?;
        if manifest.format_version != FORMAT_VERSION {
            bail!(
                "dataset format v{} but this build reads v{FORMAT_VERSION}",
                manifest.format_version
            );
        }
        Ok(Self { root })
    }

    pub fn add(
        &self,
        url: &str,
        viewport_w: i64,
        viewport_h: i64,
        dpr: f64,
        labels: Vec<ElementLabel>,
        png: &[u8],
    ) -> Result<AddOutcome> {
        let id = hex16(&Sha256::digest(png));
        let meta_path = self.meta_path(&id);
        if meta_path.exists() {
            return Ok(AddOutcome { id, deduped: true });
        }
        fs::write(self.png_path(&id), png)?;
        let meta = SampleMeta {
            id: id.clone(),
            url: url.to_owned(),
            viewport_w,
            viewport_h,
            dpr,
            captured_at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            labels,
        };
        // Meta is written after the png: a sidecar implies its image exists.
        fs::write(meta_path, serde_json::to_vec_pretty(&meta)?)?;
        Ok(AddOutcome { id, deduped: false })
    }

    pub fn sample_ids(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        for entry in fs::read_dir(self.root.join("samples"))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                ids.push(stem.to_owned());
            }
        }
        ids.sort();
        Ok(ids)
    }

    pub fn meta(&self, id: &str) -> Result<SampleMeta> {
        Ok(serde_json::from_slice(&fs::read(self.meta_path(id))?)?)
    }

    pub fn png_path(&self, id: &str) -> PathBuf {
        self.root.join("samples").join(format!("{id}.png"))
    }

    /// Path of the metadata sidecar (for tools that move/link samples whole).
    pub fn meta_json_path(&self, id: &str) -> PathBuf {
        self.meta_path(id)
    }

    fn meta_path(&self, id: &str) -> PathBuf {
        self.root.join("samples").join(format!("{id}.json"))
    }

    pub fn stats(&self) -> Result<Stats> {
        let mut stats = Stats::default();
        for id in self.sample_ids()? {
            let meta = self.meta(&id)?;
            stats.samples += 1;
            stats.labels += meta.labels.len();
            for label in meta.labels {
                *stats.by_role.entry(label.role).or_default() += 1;
            }
        }
        Ok(stats)
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Stats {
    pub samples: usize,
    pub labels: usize,
    pub by_role: BTreeMap<String, usize>,
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "samples: {}", self.samples)?;
        writeln!(f, "labels:  {}", self.labels)?;
        for (role, count) in &self.by_role {
            writeln!(f, "  {role}: {count}")?;
        }
        Ok(())
    }
}

fn hex16(digest: &[u8]) -> String {
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(role: &str) -> ElementLabel {
        ElementLabel {
            bbox: Bbox {
                x: 1.0,
                y: 2.0,
                w: 30.0,
                h: 40.0,
            },
            role: role.into(),
            name: Some("thing".into()),
        }
    }

    #[test]
    fn round_trips_a_sample() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let ds = Dataset::create(dir.path())?;
        let out = ds.add(
            "http://x/",
            1280,
            800,
            1.0,
            vec![label("button"), label("link")],
            b"fake png bytes",
        )?;
        assert!(!out.deduped);

        let ds = Dataset::open(dir.path())?;
        let ids = ds.sample_ids()?;
        assert_eq!(ids, vec![out.id.clone()]);
        let meta = ds.meta(&out.id)?;
        assert_eq!(meta.url, "http://x/");
        assert_eq!(meta.labels.len(), 2);
        assert_eq!(meta.labels[0].bbox.w, 30.0);
        assert!(ds.png_path(&out.id).exists());
        Ok(())
    }

    #[test]
    fn dedupes_identical_screenshots() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let ds = Dataset::create(dir.path())?;
        let first = ds.add("http://a/", 1280, 800, 1.0, vec![label("button")], b"same png")?;
        let second = ds.add("http://b/", 1280, 800, 1.0, vec![label("link")], b"same png")?;
        assert!(!first.deduped);
        assert!(second.deduped);
        assert_eq!(first.id, second.id);
        assert_eq!(ds.sample_ids()?.len(), 1);
        // First capture wins, the dupe's labels are dropped.
        assert_eq!(ds.meta(&first.id)?.labels[0].role, "button");
        Ok(())
    }

    #[test]
    fn rejects_unknown_format_version() -> Result<()> {
        let dir = tempfile::tempdir()?;
        Dataset::create(dir.path())?;
        fs::write(
            dir.path().join("dataset.json"),
            br#"{"format_version": 99}"#,
        )?;
        assert!(Dataset::open(dir.path()).is_err());
        Ok(())
    }

    #[test]
    fn pair_round_trip_and_click_aware_dedup() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let ds = PairDataset::create(dir.path())?;
        let signals = EffectSignals {
            dom_mutations: 3,
            aria_mutations: 1,
            ..Default::default()
        };
        let first = ds.add(
            "http://x/", Some((10.0, 20.0)), 1280, 800, 1.0, 400,
            signals, EffectSignals::default(), b"before", b"after",
        )?;
        assert!(!first.deduped);
        // Same everything -> dedupe; same images, different click -> new pair.
        assert!(ds.add(
            "http://x/", Some((10.0, 20.0)), 1280, 800, 1.0, 400,
            signals, EffectSignals::default(), b"before", b"after",
        )?.deduped);
        assert!(!ds.add(
            "http://x/", Some((99.0, 99.0)), 1280, 800, 1.0, 400,
            EffectSignals::default(), EffectSignals::default(), b"before", b"after",
        )?.deduped);

        let ds = PairDataset::open(dir.path())?;
        assert_eq!(ds.pair_ids()?.len(), 2);
        let meta = ds.meta(&first.id)?;
        assert_eq!(meta.label, EffectLabel::Changed);
        assert_eq!(meta.signals.dom_mutations, 3);
        assert_eq!(fs::read(ds.before_path(&first.id))?, b"before");
        Ok(())
    }

    #[test]
    fn labels_follow_the_signal_floor() {
        assert_eq!(
            label_from_signals(&EffectSignals::default()),
            EffectLabel::NoChange
        );
        for signals in [
            EffectSignals { dom_mutations: 1, ..Default::default() },
            EffectSignals { aria_mutations: 1, ..Default::default() },
            EffectSignals { network_requests: 1, ..Default::default() },
        ] {
            assert_eq!(label_from_signals(&signals), EffectLabel::Changed);
        }
    }

    #[test]
    fn stats_count_by_role() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let ds = Dataset::create(dir.path())?;
        ds.add(
            "http://x/",
            1280,
            800,
            1.0,
            vec![label("button"), label("button"), label("link")],
            b"png one",
        )?;
        ds.add("http://y/", 1280, 800, 1.0, vec![label("tab")], b"png two")?;
        let stats = ds.stats()?;
        assert_eq!(stats.samples, 2);
        assert_eq!(stats.labels, 4);
        assert_eq!(stats.by_role["button"], 2);
        assert_eq!(stats.by_role["link"], 1);
        assert_eq!(stats.by_role["tab"], 1);
        Ok(())
    }
}

#[cfg(test)]
mod snap_tests {
    use super::*;

    fn lbl(x: f64, w: f64) -> ElementLabel {
        ElementLabel {
            bbox: Bbox { x, y: 0.0, w, h: 40.0 },
            role: "button".into(),
            name: None,
        }
    }

    #[test]
    fn snaps_to_best_overlap_and_refuses_dead_space() {
        let labels = vec![lbl(0.0, 100.0), lbl(150.0, 100.0)];
        let pred = Bbox { x: 140.0, y: 0.0, w: 100.0, h: 40.0 };
        let hit = snap_to_label(&pred, &labels, 0.5).expect("overlaps second");
        assert_eq!(hit.bbox.x, 150.0);
        // A detection over nothing must return None, not the least-far label.
        let dead = Bbox { x: 500.0, y: 200.0, w: 50.0, h: 20.0 };
        assert!(snap_to_label(&dead, &labels, 0.5).is_none());
    }

    #[test]
    fn iou_sane() {
        let a = Bbox { x: 0.0, y: 0.0, w: 10.0, h: 10.0 };
        assert!((iou(&a, &a) - 1.0).abs() < 1e-12);
        let b = Bbox { x: 20.0, y: 0.0, w: 10.0, h: 10.0 };
        assert_eq!(iou(&a, &b), 0.0);
    }
}
