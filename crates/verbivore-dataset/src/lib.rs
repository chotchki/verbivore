//! On-disk dataset format: written by the harvester, read by training. Portable by
//! design — harvest on one machine, train on another (the files ARE the interface).
//! This crate must stay browser-free: training links it, and training should never
//! pull chromiumoxide.
//!
//! Layout: `<root>/dataset.json` (format manifest) + `<root>/samples/<id>.png` and
//! `<id>.json` sidecars. Ids are content hashes of the png, so identical
//! screenshots dedupe to one sample no matter how often they're captured.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const FORMAT_VERSION: u32 = 1;

/// Bounding box in SCREENSHOT pixels (CSS px already scaled by the sample's
/// dpr); break that invariant and every label is silently misaligned.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bbox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
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
