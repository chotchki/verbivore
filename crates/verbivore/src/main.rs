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

/// Impl #1 of the EffectJudge seam: the burn diff-stack gate. Adapted HERE in
/// the glue crate so the executor never grows an ML-framework dep.
struct BurnJudge(verbivore_effect::gate::EffectGate<burn::backend::Wgpu>);

impl verbivore_executor::EffectJudge for BurnJudge {
    fn saw_change(&self, before: &[u8], after: &[u8]) -> anyhow::Result<(f64, bool)> {
        self.0.saw_change(before, after)
    }
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
    /// Crawl an app same-host BFS, proposing task-level candidate verbs
    Crawl {
        /// Verb store root
        #[arg(long)]
        verbs: PathBuf,
        /// App label override (defaults to host-port slug of the start url)
        #[arg(long)]
        app: Option<String>,
        /// Page budget
        #[arg(long, default_value_t = 10)]
        max_pages: usize,
        /// Proposal cap per page
        #[arg(long, default_value_t = 20)]
        max_per_page: usize,
        /// Extra denied url substrings (logout/delete-style defaults always apply)
        #[arg(long)]
        deny: Vec<String>,
        start_url: String,
    },
    /// Ground one intent phrase on a page into a candidate verb record
    GenerateVerb {
        /// Verb store root
        #[arg(long)]
        verbs: PathBuf,
        /// App label override
        #[arg(long)]
        app: Option<String>,
        /// Page to ground on
        #[arg(long)]
        url: String,
        /// Also run the review-accept flow immediately
        #[arg(long, default_value_t = false)]
        accept: bool,
        intent: String,
    },
    /// Review a candidate by RUNNING it; only a Passed run flips to accepted
    AcceptVerb {
        /// Verb store root
        #[arg(long)]
        verbs: PathBuf,
        #[arg(long)]
        app: String,
        #[arg(long)]
        id: String,
        /// Settle window in ms
        #[arg(long, default_value_t = 600)]
        settle_ms: u64,
    },
    /// List stored verbs for an app
    ListVerbs {
        /// Verb store root
        #[arg(long)]
        verbs: PathBuf,
        #[arg(long)]
        app: String,
    },
    /// Run a stored verb record under an execution context; nonzero exit on
    /// any breakage (typed, printed as json for the repair loop)
    RunVerb {
        /// Verb store root
        #[arg(long)]
        verbs: PathBuf,
        /// App label the verb lives under
        #[arg(long)]
        app: String,
        /// Verb id
        #[arg(long)]
        id: String,
        /// Review mode: allow candidate (unaccepted) records
        #[arg(long, default_value_t = false)]
        allow_candidate: bool,
        /// Settle window in ms
        #[arg(long, default_value_t = 600)]
        settle_ms: u64,
        /// Effect-gate checkpoint dir; the run gates signals OR visual
        #[arg(long)]
        ckpt: Option<PathBuf>,
        /// On breakage, write a diagnostic bundle under this dir
        #[arg(long)]
        diagnostics: Option<PathBuf>,
    },
    /// Repair a broken verb: re-ground the failing step's intent against the
    /// live page, patch the selector, demote to candidate, verify
    RepairVerb {
        /// Verb store root
        #[arg(long)]
        verbs: PathBuf,
        /// App label the verb lives under
        #[arg(long)]
        app: String,
        /// Verb id
        #[arg(long)]
        id: String,
        /// Settle window in ms
        #[arg(long, default_value_t = 600)]
        settle_ms: u64,
    },
    /// Sabotage harness: click each element for real, then rewired to dead
    /// pixels, and check the signals-OR-visual gate notices the difference
    Sabotage {
        /// Trained gate checkpoint dir (effect-model.mpk + effect-model.json)
        #[arg(long)]
        ckpt: PathBuf,
        /// Settle window in ms
        #[arg(long, default_value_t = 600)]
        settle_ms: u64,
        /// Pages to sabotage
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
        Cmd::Crawl {
            verbs,
            app,
            max_pages,
            max_per_page,
            deny,
            start_url,
        } => {
            let store = verbivore_verb::VerbStore::open(verbs)?;
            let app = app.unwrap_or_else(|| verbivore_generator::app_label(&start_url));
            let mut denied: Vec<String> = verbivore_generator::crawl::DEFAULT_DENY
                .iter()
                .map(|s| s.to_string())
                .collect();
            denied.extend(deny);
            let harvester = Harvester::launch().await?;
            let report = verbivore_generator::crawl::crawl(
                &harvester,
                &store,
                &app,
                &start_url,
                max_pages,
                max_per_page,
                &denied,
            )
            .await?;
            harvester.close().await?;
            println!(
                "{app}: {} pages, {} candidates proposed, {} already stored",
                report.pages, report.proposed, report.skipped_existing
            );
        }
        Cmd::GenerateVerb {
            verbs,
            app,
            url,
            accept,
            intent,
        } => {
            let store = verbivore_verb::VerbStore::open(verbs)?;
            let app = app.unwrap_or_else(|| verbivore_generator::app_label(&url));
            let harvester = Harvester::launch().await?;
            let record =
                verbivore_generator::generate::generate(&harvester, &store, &app, &url, &intent)
                    .await?;
            harvester.close().await?;
            println!(
                "{}: candidate written ({} step(s), grounded by {})",
                record.id,
                record.steps.len(),
                record.provenance.grounded_by
            );
            if accept {
                let executor = verbivore_executor::Executor::launch().await?;
                let outcome = verbivore_executor::accept::review_and_accept(
                    &executor,
                    &store,
                    &app,
                    &record.id,
                    &verbivore_executor::ExecutionContext::default(),
                )
                .await?;
                executor.close().await?;
                println!("review: {}", serde_json::to_string(&outcome)?);
            }
        }
        Cmd::AcceptVerb {
            verbs,
            app,
            id,
            settle_ms,
        } => {
            let store = verbivore_verb::VerbStore::open(verbs)?;
            let ctx = verbivore_executor::ExecutionContext {
                settle_ms,
                ..Default::default()
            };
            let executor = verbivore_executor::Executor::launch().await?;
            let outcome =
                verbivore_executor::accept::review_and_accept(&executor, &store, &app, &id, &ctx)
                    .await?;
            executor.close().await?;
            println!("{}", serde_json::to_string_pretty(&outcome)?);
            if matches!(
                outcome,
                verbivore_executor::accept::AcceptOutcome::Rejected { .. }
            ) {
                anyhow::bail!("verb {id} failed review");
            }
        }
        Cmd::ListVerbs { verbs, app } => {
            let store = verbivore_verb::VerbStore::open(verbs)?;
            for record in store.list(&app)? {
                println!(
                    "{:9} {:40} {} step(s)  \"{}\"",
                    format!("{:?}", record.status).to_lowercase(),
                    record.id,
                    record.steps.len(),
                    record.intent
                );
            }
        }
        Cmd::RunVerb {
            verbs,
            app,
            id,
            allow_candidate,
            settle_ms,
            ckpt,
            diagnostics,
        } => {
            let store = verbivore_verb::VerbStore::open(verbs)?;
            let record = store.load(&app, &id)?;
            let ctx = verbivore_executor::ExecutionContext {
                settle_ms,
                allow_candidates: allow_candidate,
                ..Default::default()
            };
            let mut executor = verbivore_executor::Executor::launch().await?;
            if let Some(ckpt) = ckpt {
                let device = Default::default();
                executor.judge = Some(Box::new(BurnJudge(
                    verbivore_effect::gate::EffectGate::<burn::backend::Wgpu>::load(
                        &ckpt, &device,
                    )?,
                )));
            }
            let run = executor.run(&record, &ctx).await?;
            executor.close().await?;
            for (i, step) in run.steps.iter().enumerate() {
                println!(
                    "step {i}: {:?} at {:?} -> {:?} visual={:?} ({:?})",
                    step.action, step.clicked, step.effect_label, step.visual, step.signals
                );
            }
            match &run.verdict {
                verbivore_executor::RunVerdict::Passed => println!("{id}: PASSED"),
                verbivore_executor::RunVerdict::Broken { breakage } => {
                    println!("{id}: BROKEN {}", serde_json::to_string(breakage)?);
                    if let Some(dir) = diagnostics {
                        let bundle = verbivore_executor::write_diagnostics(&run, dir)?;
                        println!("diagnostics -> {}", bundle.display());
                    }
                    anyhow::bail!("verb {id} broke");
                }
            }
        }
        Cmd::RepairVerb {
            verbs,
            app,
            id,
            settle_ms,
        } => {
            let store = verbivore_verb::VerbStore::open(verbs)?;
            let ctx = verbivore_executor::ExecutionContext {
                settle_ms,
                ..Default::default()
            };
            let executor = verbivore_executor::Executor::launch().await?;
            let outcome =
                verbivore_executor::repair::repair_verb(&executor, &store, &app, &id, &ctx)
                    .await?;
            executor.close().await?;
            println!("{}", serde_json::to_string_pretty(&outcome)?);
            if matches!(
                outcome,
                verbivore_executor::repair::RepairOutcome::Unrepairable { .. }
            ) {
                anyhow::bail!("verb {id} is not repairable by re-grounding");
            }
        }
        Cmd::Sabotage {
            ckpt,
            settle_ms,
            urls,
        } => {
            let device = Default::default();
            let gate = verbivore_effect::gate::EffectGate::<burn::backend::Wgpu>::load(
                &ckpt, &device,
            )?;
            let harvester = Harvester::launch().await?;
            let mut missed = 0usize;
            for url in &urls {
                let snap = harvester.snapshot(url).await?;
                let dead = *verbivore_harvester::dead_click_points(&snap.labels, 1)
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("no dead pixel found on {url}"))?;
                println!("{url} (rewire target {:.0},{:.0}):", dead.0, dead.1);
                for label in &snap.labels {
                    let center = (
                        label.bbox.x + label.bbox.w / 2.0,
                        label.bbox.y + label.bbox.h / 2.0,
                    );
                    // The verb as recorded vs the verb after ui drift: same
                    // intent, clicks land on dead pixels.
                    let mut verdicts = Vec::new();
                    for click in [center, dead] {
                        let pair = harvester
                            .capture_action_pair(url, Some(click), settle_ms)
                            .await?;
                        let signals =
                            verbivore_dataset::label_from_signals(&pair.signals)
                                == verbivore_dataset::EffectLabel::Changed;
                        let (score, visual) =
                            gate.saw_change(&pair.before_png, &pair.after_png)?;
                        verdicts.push((signals || visual, signals, score));
                    }
                    let (true_hit, dead_hit) = (verdicts[0].0, verdicts[1].0);
                    let status = match (true_hit, dead_hit) {
                        (true, false) => "DETECTED",
                        (true, true) => {
                            missed += 1;
                            "MISSED"
                        }
                        // An element with no observable effect can't lose one.
                        (false, _) => "no-effect",
                    };
                    println!(
                        "  {status:>9} {} \"{}\": true(signals={} score={:.3}) dead(signals={} score={:.3})",
                        label.role,
                        label.name.as_deref().unwrap_or("-"),
                        verdicts[0].1,
                        verdicts[0].2,
                        verdicts[1].1,
                        verdicts[1].2,
                    );
                }
            }
            harvester.close().await?;
            anyhow::ensure!(missed == 0, "{missed} sabotaged click(s) went undetected");
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
