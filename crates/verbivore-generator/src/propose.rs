//! Page map -> candidate verb records: the crawler's brain, pure and
//! browser-free so the proposal rules are unit-testable.
//!
//! Task shaping rules (v1, deliberately conservative — a candidate that
//! never should have existed wastes a human's review attention):
//! - A NAMED form with text inputs and a button becomes ONE task verb
//!   ("submit login") — type into every field, click submit. Unnamed forms
//!   are skipped: "submit " is not an intent a human can review.
//! - Links become "open X" verbs only when they live in structural
//!   containers (nav, menu, banner, tabs) — content-area link soup
//!   (wikis!) would drown review in noise.
//! - Standalone named buttons become "press X" verbs.

use verbivore_harvester::{ContainerInfo, ElementLabel, LabeledElement};
use verbivore_verb::{
    Action, EffectExpectation, FORMAT_VERSION, Provenance, RenderingContext, RenderingVariant,
    Selector, Step, StepEvidence, TargetSpec, VerbRecord, VerbStatus, selector_for,
};

/// Containers whose links are navigation-shaped rather than content links.
const NAV_CONTAINERS: &[&str] = &["navigation", "menu", "menubar", "tablist", "banner"];

/// Text typed into generated form fields; review replaces it with something
/// task-appropriate before accepting.
const PLACEHOLDER_TEXT: &str = "verbivore";

/// Roles a form verb types into.
const TYPABLE: &[&str] = &["textbox", "searchbox"];

/// Submit-ish button name fragments, checked lowercase.
const SUBMITTY: &[&str] = &["submit", "sign", "log", "save", "search", "create", "add", "go"];

pub struct ProposalContext<'a> {
    pub app: &'a str,
    pub url: &'a str,
    /// The rendering the page map was extracted under.
    pub rendering: RenderingContext,
    /// ISO-8601 authoring timestamp.
    pub created_at: &'a str,
    /// Cap on proposals from one page — a pathological page must not flood
    /// the store.
    pub max_per_page: usize,
}

/// All labels flat, for selector disambiguation (nth needs the full page).
fn flat(elements: &[LabeledElement]) -> Vec<ElementLabel> {
    elements.iter().map(|e| e.label.clone()).collect()
}

pub fn propose(elements: &[LabeledElement], ctx: &ProposalContext) -> Vec<VerbRecord> {
    let all = flat(elements);
    let mut records: Vec<VerbRecord> = Vec::new();
    let push = |record: VerbRecord, records: &mut Vec<VerbRecord>| {
        if records.len() < ctx.max_per_page && !records.iter().any(|r| r.id == record.id) {
            records.push(record);
        }
    };

    // Form verbs first: the richest tasks get the proposal budget.
    for (form, members) in named_forms(elements) {
        let inputs: Vec<&LabeledElement> = members
            .iter()
            .copied()
            .filter(|e| TYPABLE.contains(&e.label.role.as_str()))
            .collect();
        let Some(button) = pick_submit(&members) else {
            continue;
        };
        let form_name = form.name.clone().expect("named_forms filters unnamed");
        let container_intent = format!("{} {}", form_name.to_lowercase(), form.role);
        let mut steps: Vec<Step> = Vec::new();
        for input in &inputs {
            steps.push(Step {
                action: Action::Type,
                target: Some(target(input, &container_intent, &all, field_intent(input))),
                text: Some(PLACEHOLDER_TEXT.into()),
                // Typing changes the VALUE property — invisible to the
                // MutationObserver on plain HTML forms. Don't gate on it.
                expect: EffectExpectation::DontCare,
            });
        }
        steps.push(Step {
            action: Action::Click,
            target: Some(target(button, &container_intent, &all, "submit button".into())),
            text: None,
            expect: EffectExpectation::Change,
        });
        let evidence = inputs
            .iter()
            .chain([&button])
            .map(|e| {
                Some(StepEvidence {
                    bbox: e.label.bbox,
                    score: 1.0, // a11y-grounded, not model-scored
                })
            })
            .collect();
        push(
            record(format!("submit {}", form_name.to_lowercase()), steps, evidence, ctx),
            &mut records,
        );
    }

    for element in elements {
        let Some(name) = &element.label.name else {
            continue;
        };
        if name.trim().is_empty() {
            continue;
        }
        let intent_verb = match element.label.role.as_str() {
            "link" if in_nav(element) => "open",
            "button" if !in_form(element) => "press",
            _ => continue,
        };
        let step = Step {
            action: Action::Click,
            target: Some(TargetSpec {
                container: element.container.as_ref().and_then(container_intent),
                intent: format!("the {} {}", name.to_lowercase(), element.label.role),
                selector: selector_for(&element.label, &all),
            }),
            text: None,
            expect: EffectExpectation::Change,
        };
        let evidence = vec![Some(StepEvidence { bbox: element.label.bbox, score: 1.0 })];
        push(
            record(
                format!("{intent_verb} {}", name.to_lowercase()),
                vec![step],
                evidence,
                ctx,
            ),
            &mut records,
        );
    }
    records
}

fn container_intent(c: &ContainerInfo) -> Option<String> {
    c.name
        .as_ref()
        .map(|n| format!("{} {}", n.to_lowercase(), c.role))
}

fn in_nav(e: &LabeledElement) -> bool {
    e.container
        .as_ref()
        .is_some_and(|c| NAV_CONTAINERS.contains(&c.role.as_str()))
}

fn in_form(e: &LabeledElement) -> bool {
    e.container.as_ref().is_some_and(|c| c.role == "form")
}

fn field_intent(input: &LabeledElement) -> String {
    match &input.label.name {
        Some(n) => format!("the {} field", n.to_lowercase()),
        None => "the text field".into(),
    }
}

fn target(
    element: &LabeledElement,
    container: &str,
    all: &[ElementLabel],
    intent: String,
) -> TargetSpec {
    TargetSpec {
        container: Some(container.to_string()),
        intent,
        selector: selector_for(&element.label, all),
    }
}

/// NAMED form containers with their member elements, page order.
fn named_forms<'e>(
    elements: &'e [LabeledElement],
) -> Vec<(ContainerInfo, Vec<&'e LabeledElement>)> {
    let mut forms: Vec<(ContainerInfo, Vec<&LabeledElement>)> = Vec::new();
    for element in elements {
        let Some(container) = &element.container else {
            continue;
        };
        if container.role != "form"
            || !container.name.as_ref().is_some_and(|n| !n.trim().is_empty())
        {
            continue;
        }
        match forms.iter_mut().find(|(c, _)| c == container) {
            Some((_, members)) => members.push(element),
            None => forms.push((container.clone(), vec![element])),
        }
    }
    forms
}

/// The button a form verb clicks: prefer submit-ish names, else the first.
fn pick_submit<'e>(members: &[&'e LabeledElement]) -> Option<&'e LabeledElement> {
    let buttons: Vec<&&LabeledElement> =
        members.iter().filter(|e| e.label.role == "button").collect();
    buttons
        .iter()
        .find(|b| {
            b.label
                .name
                .as_ref()
                .is_some_and(|n| SUBMITTY.iter().any(|s| n.to_lowercase().contains(s)))
        })
        .or_else(|| buttons.first())
        .map(|b| **b)
}

fn record(
    intent: String,
    steps: Vec<Step>,
    evidence: Vec<Option<StepEvidence>>,
    ctx: &ProposalContext,
) -> VerbRecord {
    VerbRecord {
        format_version: FORMAT_VERSION,
        id: slug(&intent),
        intent,
        app: ctx.app.to_string(),
        start_url: ctx.url.to_string(),
        status: VerbStatus::Candidate,
        steps,
        assertions: Vec::new(), // review's job: candidates propose, humans constrain
        provenance: Provenance {
            created_at: ctx.created_at.to_string(),
            source_url: ctx.url.to_string(),
            grounded_by: "crawler-a11y@v1".into(),
            notes: None,
        },
        variants: vec![RenderingVariant { context: ctx.rendering.clone(), evidence }],
    }
}

/// Intent -> kebab slug id, capped so pathological names stay filesystem-sane.
pub fn slug(intent: &str) -> String {
    let mut out = String::new();
    for c in intent.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 60 {
            break;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() { "unnamed".into() } else { trimmed.into() }
}

/// Selector reused by generate-verb; kept here so both paths snap identically.
pub fn selector_of(element: &LabeledElement, all: &[LabeledElement]) -> Selector {
    selector_for(&element.label, &flat(all))
}

#[cfg(test)]
mod tests {
    use super::*;
    use verbivore_dataset::Bbox;

    fn el(role: &str, name: Option<&str>, container: Option<(&str, Option<&str>)>, y: f64) -> LabeledElement {
        LabeledElement {
            label: ElementLabel {
                bbox: Bbox { x: 10.0, y, w: 120.0, h: 30.0 },
                role: role.into(),
                name: name.map(Into::into),
            },
            container: container.map(|(r, n)| ContainerInfo {
                role: r.into(),
                name: n.map(Into::into),
            }),
        }
    }

    fn ctx<'a>() -> ProposalContext<'a> {
        ProposalContext {
            app: "test-app",
            url: "http://test/login",
            rendering: RenderingContext {
                viewport_w: 1280,
                viewport_h: 800,
                dpr: 1.0,
                zoom: 1.0,
                color_scheme: "light".into(),
            },
            created_at: "2026-07-23T00:00:00Z",
            max_per_page: 20,
        }
    }

    #[test]
    fn named_form_becomes_one_task_verb() {
        let page = vec![
            el("textbox", Some("Username"), Some(("form", Some("Login"))), 100.0),
            el("textbox", Some("Password"), Some(("form", Some("Login"))), 140.0),
            el("button", Some("Sign In"), Some(("form", Some("Login"))), 180.0),
            el("button", Some("Do nothing"), None, 300.0),
        ];
        let records = propose(&page, &ctx());
        let submit = records.iter().find(|r| r.id == "submit-login").expect("form verb");
        assert_eq!(submit.steps.len(), 3, "two type steps + submit click");
        assert!(matches!(submit.steps[0].action, Action::Type));
        assert_eq!(submit.steps[0].expect, EffectExpectation::DontCare);
        assert_eq!(submit.steps[2].expect, EffectExpectation::Change);
        assert_eq!(
            submit.steps[2].target.as_ref().unwrap().container.as_deref(),
            Some("login form")
        );
        assert_eq!(submit.status, VerbStatus::Candidate);
        submit.validate().expect("proposals must validate");
        // The standalone button proposes separately.
        assert!(records.iter().any(|r| r.id == "press-do-nothing"));
    }

    #[test]
    fn unnamed_forms_and_content_links_are_skipped() {
        let page = vec![
            el("textbox", Some("q"), Some(("form", None)), 100.0),
            el("button", Some("Go"), Some(("form", None)), 100.0),
            el("link", Some("Some Article"), None, 300.0),
            el("link", Some("Home"), Some(("navigation", Some("Main"))), 20.0),
        ];
        let records = propose(&page, &ctx());
        let ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["open-home"], "nav link only: {ids:?}");
    }

    #[test]
    fn proposal_cap_holds() {
        let page: Vec<LabeledElement> = (0..40)
            .map(|i| el("button", Some(&format!("B{i}")), None, i as f64 * 20.0))
            .collect();
        let mut c = ctx();
        c.max_per_page = 5;
        assert_eq!(propose(&page, &c).len(), 5);
    }

    #[test]
    fn slug_is_filesystem_sane() {
        assert_eq!(slug("Submit Login!"), "submit-login");
        assert_eq!(slug("  ??? "), "unnamed");
        assert!(slug(&"x y ".repeat(100)).len() <= 60);
    }
}
