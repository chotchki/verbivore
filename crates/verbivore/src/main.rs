use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use verbivore_dataset::Dataset;
use verbivore_harvester::{Harvester, Variation};

#[derive(Parser)]
#[command(name = "verbivore", version, about = "Vision-assisted verbs for browser testing")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print sample/label counts for a harvested dataset
    DatasetStats {
        /// Dataset root (the directory holding dataset.json)
        dir: PathBuf,
    },
    /// Sweep urls through the full variation grid into a dataset
    Harvest {
        /// Dataset root to create or extend
        #[arg(long)]
        dataset: PathBuf,
        /// Pages to harvest
        urls: Vec<String>,
    },
    /// Capture effect pairs: element clicks, dead clicks and a no-action
    /// control per url, labeled from CDP signals
    HarvestPairs {
        /// Pair dataset root to create or extend
        #[arg(long)]
        pairs: PathBuf,
        /// Element clicks per page
        #[arg(long, default_value_t = 5)]
        max_elements: usize,
        /// Dead-area clicks per page
        #[arg(long, default_value_t = 2)]
        dead: usize,
        /// Settle window in ms (also the ambient control window)
        #[arg(long, default_value_t = 400)]
        settle_ms: u64,
        urls: Vec<String>,
    },
    /// Split a dataset into per-host datasets under an output root
    DatasetSplit {
        src: PathBuf,
        out_root: PathBuf,
    },
    /// Merge datasets into one via hardlinks (content-addressing dedupes)
    DatasetMerge {
        out: PathBuf,
        srcs: Vec<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::DatasetStats { dir } => {
            print!("{}", Dataset::open(dir)?.stats()?);
        }
        Cmd::Harvest { dataset, urls } => {
            let ds = Dataset::create(dataset)?;
            let harvester = Harvester::launch().await?;
            let grid = Variation::default_grid();
            for url in &urls {
                let outcome = harvester.harvest_variations(&ds, url, &grid).await?;
                println!(
                    "{url}: {} added, {} deduped",
                    outcome.added, outcome.deduped
                );
            }
            harvester.close().await?;
            print!("{}", ds.stats()?);
        }
        Cmd::HarvestPairs {
            pairs,
            max_elements,
            dead,
            settle_ms,
            urls,
        } => {
            let ds = verbivore_dataset::PairDataset::create(pairs)?;
            let harvester = Harvester::launch().await?;
            for url in &urls {
                let outcome = harvester
                    .harvest_pairs(&ds, url, max_elements, dead, settle_ms)
                    .await?;
                println!("{url}: {} added, {} deduped", outcome.added, outcome.deduped);
            }
            harvester.close().await?;
        }
        Cmd::DatasetSplit { src, out_root } => {
            let src_ds = Dataset::open(&src)?;
            for id in src_ds.sample_ids()? {
                let meta = src_ds.meta(&id)?;
                let host = host_label(&meta.url);
                let dst = Dataset::create(out_root.join(&host))?;
                link_sample(&src_ds, &dst, &id)?;
            }
            for entry in std::fs::read_dir(&out_root)? {
                let path = entry?.path();
                if path.is_dir() {
                    println!("{}:", path.display());
                    print!("{}", Dataset::open(&path)?.stats()?);
                }
            }
        }
        Cmd::DatasetMerge { out, srcs } => {
            let dst = Dataset::create(&out)?;
            for src in &srcs {
                let src_ds = Dataset::open(src)?;
                for id in src_ds.sample_ids()? {
                    link_sample(&src_ds, &dst, &id)?;
                }
            }
            print!("{}", dst.stats()?);
        }
    }
    Ok(())
}

/// "localhost:42001" -> "localhost-42001"; anything non-filename-safe becomes '-'.
fn host_label(url: &str) -> String {
    let host = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("unknown");
    host.chars()
        .map(|c| if c.is_alphanumeric() || c == '.' { c } else { '-' })
        .collect()
}

/// Hardlink a sample's pair into another dataset; existing ids are dedup hits.
fn link_sample(src: &Dataset, dst: &Dataset, id: &str) -> Result<()> {
    for (from, to) in [
        (src.png_path(id), dst.png_path(id)),
        (src.meta_json_path(id), dst.meta_json_path(id)),
    ] {
        match std::fs::hard_link(&from, &to) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
