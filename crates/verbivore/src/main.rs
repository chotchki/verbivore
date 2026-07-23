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
    }
    Ok(())
}
