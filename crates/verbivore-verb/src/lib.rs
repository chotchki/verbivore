//! Verb records: task-level browser actions as DATA, executed by a generic
//! executor — never generated code. One JSON file per verb so the repair loop
//! patches records with human-sized reviewable diffs.
//!
//! The shape encodes the project's settled calls:
//! - Task-level intents ("submit login"), steps scoped by container intents
//!   ("login form" > "submit button") — the grounding chain from the spec.
//! - Rendering is ENVIRONMENT, not verb identity: execution context is passed
//!   in at runtime, records store per-rendering evidence variants, and a
//!   context with no variant is a repair trigger, not a guess.
//! - `Action::Custom` is the quirk-registry escape hatch: the schema stays a
//!   flat sequence, control flow stays Rust.
//! - SCHEMA GUARD: no ML-framework types anywhere in this crate. Provenance
//!   identifies models by STRING. Records written today stay valid when the
//!   grounding regime changes.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use verbivore_dataset::Bbox;

pub const FORMAT_VERSION: u32 = 1;

/// A task-level verb: "submit login", not "click at (312, 480)".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerbRecord {
    pub format_version: u32,
    /// Kebab-case slug, unique within the app: "submit-login".
    pub id: String,
    /// The natural-language task intent the verb was generated from.
    pub intent: String,
    /// Host label scoping the verb to an app (dataset conventions apply).
    pub app: String,
    /// Where the verb starts; the executor navigates here first.
    pub start_url: String,
    pub status: VerbStatus,
    pub steps: Vec<Step>,
    /// Task-level expected outcomes, checked after the last step — richer
    /// than per-click pixel diffs ("the login form goes away").
    pub assertions: Vec<Assertion>,
    pub provenance: Provenance,
    /// Grounding evidence per rendering context. Runtime picks the variant
    /// matching the execution context; no match = repair trigger.
    pub variants: Vec<RenderingVariant>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum VerbStatus {
    /// Generated, not yet human-reviewed. The executor refuses these outside
    /// review mode.
    Candidate,
    Accepted,
    /// Kept for history; never executed.
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Step {
    pub action: Action,
    /// What to act on. Required for the pointer/keyboard actions; a custom
    /// action may target nothing (the registered impl owns its behavior).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetSpec>,
    /// Text to enter; `Type` only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// What the effect gate should demand after this step.
    pub expect: EffectExpectation,
}

/// Right-click is first-class: recon-gen's diagram context menus only exist
/// post-interaction, so the crawler can't reach them without it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Action {
    Click,
    RightClick,
    Hover,
    Type,
    /// Name into the quirk registry (4.3). Unregistered names fail execution,
    /// not validation — the record can travel ahead of its impl.
    Custom { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TargetSpec {
    /// Container intent narrowing the search ("login form"); None = whole page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    /// Element intent within the container ("submit button").
    pub intent: String,
    /// The snapped selector: role + accessible name, the durable address the
    /// grounder resolved to at authoring time.
    pub selector: Selector,
}

/// Role + accessible-name selector. `nth` disambiguates true duplicates
/// (the rank-top1 misses were all name+role twins).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Selector {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nth: Option<usize>,
}

/// Selector snapping, the last hop of grounding: a chosen label becomes the
/// durable role+name address a record stores. `nth` is set ONLY when the
/// page has role+name twins — an unambiguous selector must stay robust to
/// unrelated elements appearing or leaving.
pub fn selector_for(
    label: &verbivore_dataset::ElementLabel,
    all_labels: &[verbivore_dataset::ElementLabel],
) -> Selector {
    let twins: Vec<&verbivore_dataset::ElementLabel> = all_labels
        .iter()
        .filter(|l| l.role == label.role && l.name == label.name)
        .collect();
    // Twins share role+name but never a bbox — position by geometry so a
    // cloned label still finds itself.
    let nth = (twins.len() > 1)
        .then(|| twins.iter().position(|l| l.bbox == label.bbox))
        .flatten();
    Selector {
        role: label.role.clone(),
        name: label.name.clone(),
        nth,
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EffectExpectation {
    /// Write action: the signals-OR-visual gate must fire, silence = dead
    /// click = the sabotage the whole pipeline exists to catch.
    Change,
    /// The gate must stay silent (rare; guards known no-op steps).
    NoChange,
    /// Hover and friends: a tooltip may or may not appear; don't judge.
    DontCare,
}

/// Task-level postconditions, checked against the a11y tree after the final
/// step. Deliberately small; grow only when a corpus verb needs more.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Assertion {
    ElementPresent {
        #[serde(skip_serializing_if = "Option::is_none")]
        container: Option<String>,
        selector: Selector,
    },
    ElementGone {
        #[serde(skip_serializing_if = "Option::is_none")]
        container: Option<String>,
        selector: Selector,
    },
    /// Substring match on the final url.
    UrlContains { needle: String },
}

/// Who made this record and from what. Model identities are STRINGS —
/// "grounding-v2@61525ae" — never framework types (schema guard).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Provenance {
    /// ISO-8601; absolute, never relative.
    pub created_at: String,
    pub source_url: String,
    pub grounded_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// The rendering environment a variant's evidence was observed under.
/// Mirrors the harvester's variation grid without importing browser deps.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderingContext {
    pub viewport_w: i64,
    pub viewport_h: i64,
    pub dpr: f64,
    pub zoom: f64,
    /// "light" | "dark".
    pub color_scheme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderingVariant {
    pub context: RenderingContext,
    /// One entry per step (index-aligned); steps without a target carry None.
    pub evidence: Vec<Option<StepEvidence>>,
}

/// What the grounder saw at authoring time: where the target was and how
/// confident the match. The repair loop compares against this.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepEvidence {
    /// Screenshot px under the variant's context.
    pub bbox: Bbox,
    pub score: f64,
}

impl VerbRecord {
    /// Structural validation — the rules a record must satisfy to be worth
    /// storing at all. Execution-time concerns (unregistered custom actions,
    /// context misses) fail at execution, not here: records travel ahead of
    /// their impls.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.format_version == FORMAT_VERSION,
            "format_version {} (this build reads {FORMAT_VERSION})",
            self.format_version
        );
        ensure!(!self.id.is_empty(), "empty id");
        ensure!(
            self.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "id must be a kebab-case slug: {:?}",
            self.id
        );
        ensure!(!self.intent.is_empty(), "empty intent");
        ensure!(!self.app.is_empty(), "empty app");
        ensure!(!self.steps.is_empty(), "a verb needs at least one step");
        for (i, step) in self.steps.iter().enumerate() {
            let ctx = |msg: &str| format!("step {i}: {msg}");
            match &step.action {
                Action::Type => {
                    ensure!(step.text.is_some(), ctx("type without text"));
                }
                Action::Custom { name } => {
                    ensure!(!name.is_empty(), ctx("custom action with empty name"));
                }
                _ => {}
            }
            if !matches!(step.action, Action::Type) {
                ensure!(step.text.is_none(), ctx("text on a non-type action"));
            }
            if !matches!(step.action, Action::Custom { .. }) {
                let target = step.target.as_ref();
                let Some(target) = target else {
                    bail!(ctx("pointer/keyboard action without a target"));
                };
                ensure!(!target.intent.is_empty(), ctx("empty target intent"));
                ensure!(!target.selector.role.is_empty(), ctx("empty selector role"));
            }
        }
        for (i, variant) in self.variants.iter().enumerate() {
            ensure!(
                variant.evidence.len() == self.steps.len(),
                "variant {i}: {} evidence entries for {} steps",
                variant.evidence.len(),
                self.steps.len()
            );
        }
        Ok(())
    }
}

/// One JSON file per verb under `<root>/<app>/<id>.json`. Same content-first
/// philosophy as the datasets: the directory IS the database.
pub struct VerbStore {
    root: PathBuf,
}

impl VerbStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn path_for(&self, app: &str, id: &str) -> PathBuf {
        self.root.join(app).join(format!("{id}.json"))
    }

    /// Validates, then writes (atomically via tmp+rename — the repair loop
    /// rewrites records in place and a torn write is a corrupted verb).
    pub fn save(&self, record: &VerbRecord) -> Result<PathBuf> {
        record.validate()?;
        let path = self.path_for(&record.app, &record.id);
        std::fs::create_dir_all(path.parent().expect("app dir"))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(record)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    }

    pub fn load(&self, app: &str, id: &str) -> Result<VerbRecord> {
        Self::load_path(&self.path_for(app, id))
    }

    pub fn load_path(path: &Path) -> Result<VerbRecord> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let record: VerbRecord = serde_json::from_str(&raw)
            .with_context(|| format!("parsing {}", path.display()))?;
        record.validate()?;
        Ok(record)
    }

    /// All records for an app, id-sorted. Missing app dir = empty, not error.
    pub fn list(&self, app: &str) -> Result<Vec<VerbRecord>> {
        let dir = self.root.join(app);
        let mut records = Vec::new();
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(records),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") {
                records.push(Self::load_path(&path)?);
            }
        }
        records.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> VerbRecord {
        VerbRecord {
            format_version: FORMAT_VERSION,
            id: "submit-login".into(),
            intent: "submit login".into(),
            app: "gitea-42002".into(),
            start_url: "http://localhost:42002/user/login".into(),
            status: VerbStatus::Candidate,
            steps: vec![
                Step {
                    action: Action::Type,
                    target: Some(TargetSpec {
                        container: Some("login form".into()),
                        intent: "username field".into(),
                        selector: Selector {
                            role: "textbox".into(),
                            name: Some("Username or Email Address".into()),
                            nth: None,
                        },
                    }),
                    text: Some("demo".into()),
                    expect: EffectExpectation::Change,
                },
                Step {
                    action: Action::Click,
                    target: Some(TargetSpec {
                        container: Some("login form".into()),
                        intent: "submit button".into(),
                        selector: Selector {
                            role: "button".into(),
                            name: Some("Sign In".into()),
                            nth: None,
                        },
                    }),
                    text: None,
                    expect: EffectExpectation::Change,
                },
            ],
            assertions: vec![Assertion::ElementGone {
                container: None,
                selector: Selector {
                    role: "button".into(),
                    name: Some("Sign In".into()),
                    nth: None,
                },
            }],
            provenance: Provenance {
                created_at: "2026-07-23T18:00:00Z".into(),
                source_url: "http://localhost:42002/user/login".into(),
                grounded_by: "grounding-v2@61525ae".into(),
                notes: None,
            },
            variants: vec![RenderingVariant {
                context: RenderingContext {
                    viewport_w: 1280,
                    viewport_h: 800,
                    dpr: 1.0,
                    zoom: 1.0,
                    color_scheme: "light".into(),
                },
                evidence: vec![
                    Some(StepEvidence {
                        bbox: Bbox { x: 490.0, y: 300.0, w: 300.0, h: 38.0 },
                        score: 0.91,
                    }),
                    Some(StepEvidence {
                        bbox: Bbox { x: 490.0, y: 420.0, w: 120.0, h: 40.0 },
                        score: 0.97,
                    }),
                ],
            }],
        }
    }

    #[test]
    fn round_trips_through_store() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = VerbStore::open(dir.path())?;
        let record = sample();
        store.save(&record)?;
        assert_eq!(store.load("gitea-42002", "submit-login")?, record);
        assert_eq!(store.list("gitea-42002")?.len(), 1);
        assert!(store.list("no-such-app")?.is_empty());
        Ok(())
    }

    #[test]
    fn validation_rejects_the_deadly_shapes() {
        let mut bad = sample();
        bad.steps[0].text = None; // type without text
        assert!(bad.validate().is_err());

        let mut bad = sample();
        bad.steps[1].target = None; // click without target
        assert!(bad.validate().is_err());

        let mut bad = sample();
        bad.id = "Submit Login".into(); // not a slug
        assert!(bad.validate().is_err());

        let mut bad = sample();
        bad.variants[0].evidence.pop(); // evidence misaligned with steps
        assert!(bad.validate().is_err());

        let mut bad = sample();
        bad.steps[1].text = Some("stray".into()); // text on a click
        assert!(bad.validate().is_err());
    }

    #[test]
    fn custom_action_travels_without_target_or_impl() -> Result<()> {
        let mut record = sample();
        record.steps.push(Step {
            action: Action::Custom { name: "drag-date-slider".into() },
            target: None,
            text: None,
            expect: EffectExpectation::Change,
        });
        record.variants[0].evidence.push(None);
        record.validate()
    }

    #[test]
    fn json_shape_is_reviewable() -> Result<()> {
        // The repair loop's diffs are only reviewable if the serialization is
        // stable and self-describing — pin the discriminants we rely on.
        let json = serde_json::to_string_pretty(&sample())?;
        assert!(json.contains(r#""kind": "type""#));
        assert!(json.contains(r#""kind": "element-gone""#));
        assert!(json.contains(r#""expect": "change""#));
        assert!(json.contains(r#""grounded_by": "grounding-v2@61525ae""#));
        Ok(())
    }
}

#[cfg(test)]
mod snap_tests {
    use super::*;
    use verbivore_dataset::ElementLabel;

    fn label(role: &str, name: &str, x: f64) -> ElementLabel {
        ElementLabel {
            bbox: Bbox { x, y: 10.0, w: 80.0, h: 30.0 },
            role: role.into(),
            name: Some(name.into()),
        }
    }

    #[test]
    fn unique_labels_get_no_nth() {
        let labels = vec![label("button", "Save", 10.0), label("button", "Cancel", 100.0)];
        let s = selector_for(&labels[0], &labels);
        assert_eq!((s.role.as_str(), s.nth), ("button", None));
    }

    #[test]
    fn twins_get_positional_nth_even_from_clones() {
        // Two "Submit" buttons (one per form) — the container-scoping case.
        let labels = vec![
            label("button", "Submit", 10.0),
            label("button", "Cancel", 100.0),
            label("button", "Submit", 200.0),
        ];
        let cloned = labels[2].clone();
        let s = selector_for(&cloned, &labels);
        assert_eq!(s.nth, Some(1), "second Submit twin");
        assert_eq!(selector_for(&labels[0], &labels).nth, Some(0));
    }
}
