//! Generic executor: runs verb records (data, never generated code) as
//! deterministic CDP actions. Custom-action registry is the escape hatch for
//! behavior that data can't express — the schema stays a flat sequence,
//! control flow stays Rust.
//!
//! Every way a run can go wrong is a TYPED `Breakage` — the repair loop (4.6)
//! consumes these, so "it failed" is never the output; "step 2's selector
//! matched nothing" is. The first breakage aborts the run: repair wants the
//! first divergence point, not a cascade of consequences.
//!
//! Effect checking here is the SIGNALS half of the signals-OR-visual gate;
//! 4.4 adds the pair model over the captured pngs. Labels re-extract before
//! every step because menus only exist post-interaction.

pub mod repair;

use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use chromiumoxide::Page;
use futures::future::BoxFuture;
use serde::Serialize;
use verbivore_dataset::{EffectLabel, ElementLabel, label_from_signals};
use verbivore_harvester::{
    ColorScheme, Harvester, Variation,
    effect_capture::{self, AMBIENT_MIN_MS, ActionSignals},
    input,
};
use verbivore_verb::{
    Action, Assertion, EffectExpectation, RenderingContext, Selector, VerbRecord, VerbStatus,
};

/// What the caller wants the verb run under. Rendering is environment, not
/// verb identity — the record must hold a variant for it or the run is a
/// repair trigger before the browser even opens.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub rendering: RenderingContext,
    pub settle_ms: u64,
    /// Review mode: candidate records may run; accepted-only otherwise.
    pub allow_candidates: bool,
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self {
            rendering: RenderingContext {
                viewport_w: 1280,
                viewport_h: 800,
                dpr: 1.0,
                zoom: 1.0,
                color_scheme: "light".into(),
            },
            settle_ms: 600,
            allow_candidates: false,
        }
    }
}

/// Everything that can break, named. Serializable — these feed the repair
/// loop and the diagnostic bundle.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Breakage {
    NotAccepted { status: String },
    NoVariantForContext,
    TargetNotFound { step: usize, selector: Selector },
    AmbiguousTarget { step: usize, selector: Selector, count: usize },
    /// Expected a change; the signal channel stayed silent. (The visual
    /// channel gets its say in 4.4 — silence here is already suspicious.)
    EffectSilence { step: usize },
    UnexpectedEffect { step: usize },
    UnregisteredCustomAction { step: usize, name: String },
    AssertionFailed { index: usize },
}

#[derive(Debug, Serialize)]
pub struct StepReport {
    pub action: Action,
    /// CSS-px center the action landed on (None for untargeted customs).
    pub clicked: Option<(f64, f64)>,
    pub signals: ActionSignals,
    pub effect_label: EffectLabel,
    /// The visual channel's (score, saw_change) — None when no judge is set.
    pub visual: Option<(f64, bool)>,
    /// Raw pair; not serialized — the diagnostic bundle persists them as pngs.
    #[serde(skip)]
    pub before_png: Vec<u8>,
    #[serde(skip)]
    pub after_png: Vec<u8>,
}

/// The page as it was when target resolution failed — everything re-grounding
/// needs without reopening the browser.
#[derive(Debug, Serialize)]
pub struct BreakScene {
    pub labels: Vec<ElementLabel>,
    #[serde(skip)]
    pub screenshot_png: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct VerbRun {
    pub verb_id: String,
    pub steps: Vec<StepReport>,
    pub verdict: RunVerdict,
    /// Present when the verdict is a target-resolution breakage.
    pub break_scene: Option<BreakScene>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RunVerdict {
    Passed,
    Broken { breakage: Breakage },
}

/// The visual half of the effect gate, as a seam: the executor never knows
/// what's judging — burn checkpoint, ONNX import, cloud call. Local models
/// are impl #1 (the CLI adapts `verbivore-effect`'s gate to this). The trait
/// is the boundary where optionality STOPS: inside an impl, commit to a stack.
pub trait EffectJudge: Send + Sync {
    /// (score, saw_change) for a before/after png pair.
    fn saw_change(&self, before_png: &[u8], after_png: &[u8]) -> Result<(f64, bool)>;
}

/// A registered quirk impl: full page access, owns its own waiting.
pub type CustomAction =
    Box<dyn for<'a> Fn(&'a Page) -> BoxFuture<'a, Result<()>> + Send + Sync>;

#[derive(Default)]
pub struct CustomRegistry {
    actions: HashMap<String, CustomAction>,
}

impl CustomRegistry {
    pub fn register(&mut self, name: impl Into<String>, action: CustomAction) {
        self.actions.insert(name.into(), action);
    }

    pub fn get(&self, name: &str) -> Option<&CustomAction> {
        self.actions.get(name)
    }
}

pub struct Executor {
    harvester: Harvester,
    pub registry: CustomRegistry,
    /// The visual channel. None = signals-only gating (canvas effects
    /// invisible); the runtime gate is signals OR visual when present.
    pub judge: Option<Box<dyn EffectJudge>>,
}

/// Persists a run for postmortem under `<dir>/<verb_id>/`: run.json (verdict,
/// per-step signals + visual scores) plus the step-N before/after pngs — what
/// a human (or the repair loop) needs to see what the gate saw.
pub fn write_diagnostics(run: &VerbRun, dir: impl AsRef<std::path::Path>) -> Result<std::path::PathBuf> {
    let bundle = dir.as_ref().join(&run.verb_id);
    std::fs::create_dir_all(&bundle)?;
    std::fs::write(bundle.join("run.json"), serde_json::to_string_pretty(run)?)?;
    for (i, step) in run.steps.iter().enumerate() {
        std::fs::write(bundle.join(format!("step-{i}.before.png")), &step.before_png)?;
        std::fs::write(bundle.join(format!("step-{i}.after.png")), &step.after_png)?;
    }
    Ok(bundle)
}

fn variation_for(rendering: &RenderingContext) -> Result<Variation> {
    Ok(Variation {
        viewport: (rendering.viewport_w, rendering.viewport_h),
        zoom: rendering.zoom,
        dpr: rendering.dpr,
        color_scheme: match rendering.color_scheme.as_str() {
            "light" => ColorScheme::Light,
            "dark" => ColorScheme::Dark,
            other => return Err(anyhow!("unknown color scheme {other:?}")),
        },
    })
}

fn context_matches(a: &RenderingContext, b: &RenderingContext) -> bool {
    a.viewport_w == b.viewport_w
        && a.viewport_h == b.viewport_h
        && a.dpr == b.dpr
        && a.zoom == b.zoom
        && a.color_scheme == b.color_scheme
}

/// Resolve a role+name selector against current labels. Labels keep a11y-tree
/// order, so `nth` is deterministic run to run.
fn resolve<'l>(
    labels: &'l [ElementLabel],
    selector: &Selector,
) -> std::result::Result<&'l ElementLabel, ResolveMiss> {
    let matches: Vec<&ElementLabel> = labels
        .iter()
        .filter(|l| l.role == selector.role)
        .filter(|l| match &selector.name {
            Some(name) => l.name.as_deref() == Some(name.as_str()),
            None => true,
        })
        .collect();
    match (matches.len(), selector.nth) {
        (0, _) => Err(ResolveMiss::NotFound),
        (1, None) => Ok(matches[0]),
        (n, None) => Err(ResolveMiss::Ambiguous { count: n }),
        (n, Some(i)) if i < n => Ok(matches[i]),
        (_, Some(_)) => Err(ResolveMiss::NotFound),
    }
}

enum ResolveMiss {
    NotFound,
    Ambiguous { count: usize },
}

impl Executor {
    pub async fn launch() -> Result<Self> {
        Ok(Self {
            harvester: Harvester::launch().await?,
            registry: CustomRegistry::default(),
            judge: None,
        })
    }

    pub async fn close(self) -> Result<()> {
        self.harvester.close().await
    }

    pub async fn run(&self, record: &VerbRecord, ctx: &ExecutionContext) -> Result<VerbRun> {
        record.validate()?;
        let broken = |steps: Vec<StepReport>, breakage: Breakage| VerbRun {
            verb_id: record.id.clone(),
            steps,
            verdict: RunVerdict::Broken { breakage },
            break_scene: None,
        };

        if record.status != VerbStatus::Accepted && !ctx.allow_candidates {
            return Ok(broken(
                Vec::new(),
                Breakage::NotAccepted { status: format!("{:?}", record.status) },
            ));
        }
        // Rendering context must have authored evidence — otherwise this
        // rendering has never been grounded and running would be guessing.
        if !record.variants.iter().any(|v| context_matches(&v.context, &ctx.rendering)) {
            return Ok(broken(Vec::new(), Breakage::NoVariantForContext));
        }

        let variation = variation_for(&ctx.rendering)?;
        let page = self.harvester.open_page(&record.start_url, &variation).await?;
        let run = self.run_on(&page, record, ctx, &variation).await;
        page.close().await.ok();
        run
    }

    async fn run_on(
        &self,
        page: &Page,
        record: &VerbRecord,
        ctx: &ExecutionContext,
        variation: &Variation,
    ) -> Result<VerbRun> {
        let mut steps: Vec<StepReport> = Vec::new();
        let broken = |steps: Vec<StepReport>, breakage: Breakage| {
            Ok(VerbRun {
                verb_id: record.id.clone(),
                steps,
                verdict: RunVerdict::Broken { breakage },
                break_scene: None,
            })
        };

        for (i, step) in record.steps.iter().enumerate() {
            // Target resolution against the page AS IT IS NOW.
            let click_css = match (&step.action, &step.target) {
                (Action::Custom { .. }, None) => None,
                (_, Some(target)) => {
                    let labels = self.harvester.labels_on(page, variation).await?;
                    let label = match resolve(&labels, &target.selector) {
                        Ok(l) => l,
                        Err(miss) => {
                            let breakage = match miss {
                                ResolveMiss::NotFound => Breakage::TargetNotFound {
                                    step: i,
                                    selector: target.selector.clone(),
                                },
                                ResolveMiss::Ambiguous { count } => Breakage::AmbiguousTarget {
                                    step: i,
                                    selector: target.selector.clone(),
                                    count,
                                },
                            };
                            // Capture the scene the failure happened in —
                            // re-grounding wants THIS page, not a fresh load.
                            let screenshot_png = effect_capture::shot(page).await?;
                            return Ok(VerbRun {
                                verb_id: record.id.clone(),
                                steps,
                                verdict: RunVerdict::Broken { breakage },
                                break_scene: Some(BreakScene { labels, screenshot_png }),
                            });
                        }
                    };
                    // Labels are screenshot px; dispatch wants CSS px.
                    Some((
                        (label.bbox.x + label.bbox.w / 2.0) / variation.dpr,
                        (label.bbox.y + label.bbox.h / 2.0) / variation.dpr,
                    ))
                }
                (action, None) => {
                    return Err(anyhow!("validated record lost its target: {action:?} step {i}"));
                }
            };

            // Observe ambient, act, read what the action did.
            effect_capture::arm(page).await?;
            tokio::time::sleep(std::time::Duration::from_millis(
                ctx.settle_ms.max(AMBIENT_MIN_MS),
            ))
            .await;
            effect_capture::begin_action(page).await?;
            let before_png = effect_capture::shot(page).await?;
            match (&step.action, click_css) {
                (Action::Click, Some((x, y))) => input::click_at(page, x, y).await?,
                (Action::RightClick, Some((x, y))) => input::right_click_at(page, x, y).await?,
                (Action::Hover, Some((x, y))) => input::hover_at(page, x, y).await?,
                (Action::Type, Some((x, y))) => {
                    input::type_at(page, x, y, step.text.as_deref().unwrap_or_default()).await?;
                }
                (Action::Custom { name }, _) => match self.registry.get(name) {
                    Some(action) => action(page).await.with_context(|| format!("custom {name}"))?,
                    None => {
                        return broken(
                            steps,
                            Breakage::UnregisteredCustomAction { step: i, name: name.clone() },
                        );
                    }
                },
                (action, None) => {
                    return Err(anyhow!("unreachable: {action:?} without coordinates"));
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(ctx.settle_ms)).await;
            let after_png = effect_capture::shot(page).await?;
            let signals = effect_capture::read_action(page).await?;
            let effect_label = label_from_signals(&signals);
            let visual = match &self.judge {
                Some(judge) => Some(
                    judge
                        .saw_change(&before_png, &after_png)
                        .with_context(|| format!("effect judge, step {i}"))?,
                ),
                None => None,
            };

            let report = StepReport {
                action: step.action.clone(),
                clicked: click_css,
                signals: signals.clone(),
                effect_label,
                visual,
                before_png,
                after_png,
            };
            let navigated = signals.navigated;
            steps.push(report);

            // The gate is deliberately asymmetric. Change: signals OR visual —
            // either channel proves life, silence on both = dead click.
            // NoChange: signals ONLY — proving stillness visually on an
            // animated page is where the model is weakest (focus rings sit
            // right at its threshold; measured live on the fixture), and a
            // borderline visual score must not break a signal-verified no-op.
            match step.expect {
                EffectExpectation::Change => {
                    let alive = effect_label == EffectLabel::Changed
                        || visual.is_some_and(|(_, saw)| saw);
                    if !alive {
                        return broken(steps, Breakage::EffectSilence { step: i });
                    }
                }
                EffectExpectation::NoChange => {
                    if effect_label == EffectLabel::Changed {
                        return broken(steps, Breakage::UnexpectedEffect { step: i });
                    }
                }
                EffectExpectation::DontCare => {}
            }
            if navigated {
                // The document was replaced; give the new one a beat to land
                // before the next step re-extracts labels.
                page.wait_for_navigation().await.ok();
            }
        }

        for (i, assertion) in record.assertions.iter().enumerate() {
            let ok = match assertion {
                Assertion::ElementPresent { selector, .. } => {
                    let labels = self.harvester.labels_on(page, variation).await?;
                    resolve(&labels, selector).is_ok()
                }
                Assertion::ElementGone { selector, .. } => {
                    let labels = self.harvester.labels_on(page, variation).await?;
                    matches!(resolve(&labels, selector), Err(ResolveMiss::NotFound))
                }
                Assertion::UrlContains { needle } => {
                    page.url().await?.is_some_and(|u| u.contains(needle.as_str()))
                }
            };
            if !ok {
                return broken(steps, Breakage::AssertionFailed { index: i });
            }
        }

        Ok(VerbRun {
            verb_id: record.id.clone(),
            steps,
            verdict: RunVerdict::Passed,
            break_scene: None,
        })
    }
}
