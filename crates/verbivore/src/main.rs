use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use verbivore_dataset::Dataset;

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
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::DatasetStats { dir } => {
            print!("{}", Dataset::open(dir)?.stats()?);
        }
    }
    Ok(())
}
