<!-- plan-bridge:phase-high-water=A -->
# PLAN

Vision-assisted verbs for browser testing — [SPEC.md](SPEC.md) owns the what and why. Phases: harvest → ground → validate → execute → generate, canvas and friends live in the backlog until v1 ships.

<!--
This PLAN.md is driven by `claude-plan-bridge` (FORMATv2):
- Phases are `## Phase <ID> - <Title>` headers; tasks are `- [ ] <ID> - <task>`
  lines under them.
- TaskCreate adds a task line at `metadata.plan_path`; with no `plan_path` it
  lands as a tracked note in the bottom `# Backlog (not yet phased)` section.
- TaskUpdate(status='completed') ticks the box; (status='deleted') removes
  the line; (subject='...') rewrites the title.
- Hand-edits between turns surface as `additionalContext` on the next
  prompt — the bridge reconciles on every UserPromptSubmit.
- `claude-plan-bridge archive` sweeps fully-`[x]` top-level phases into
  PLAN_ARCHIVE.md.
- `claude-plan-bridge status` reports state-file health if something
  looks wrong.
-->

## Phase 4 - Verb records + executor
- [x] 4.1 - Verb schema: action enum (click, right-click, hover, type — recon-gen's diagram context menus need right-click; menus only exist post-interaction), container-scoped intents, assertions, provenance block. verbivore-verb crate: browser-free AND burn-free (the schema guard is structural — no dep to leak), one json per verb for reviewable repair diffs, per-rendering evidence variants (context miss = repair trigger), Action::Custom as the 4.3 hook, atomic saves
- [ ] 4.2 - Generic executor: primitive actions over chromiumoxide
- [ ] 4.3 - Custom-action registry (the quirk escape hatch)
- [ ] 4.4 - Effect gate: pair model as settle check + diagnostic bundle on failure
- [ ] 4.5 - Selector snapping: grounded element to role + accessible-name selector
- [ ] 4.6 - Repair loop: detect breakage, re-ground, patch record with reviewable diff

## Phase 5 - Generation + end-to-end
- [ ] 5.1 - Crawler: walk a corpus app, propose task-level candidate verbs with scoped intents
- [ ] 5.2 - Candidate review + accept flow (status flip on the record)
- [ ] 5.3 - generate-verb CLI: intent phrase to accepted verb record
- [ ] 5.4 - End-to-end: generated verb runs deterministically, sabotage self-reports
- [ ] 5.5 - README + verb-format doc, sweep phase to archive

# Backlog (not yet phased)

- Canvas verbs: runtime grounding + coordinate actions for canvas content (QuickSight accounts are dead; stand-ins: Grafana's uPlot panels are ALREADY canvas, add an ECharts app like Superset for depth)
- Public-site harvesting for corpus diversity — MEASURED MOTIVATION (v4, 2026-07-23): +100 epochs and +18 deep-page samples replicated v3 (mAP 0.032 vs 0.040, loss 0.92->0.60 = overfit bending), same-app page mining dedupes to nothing; NEW APPS are the only remaining lever. Sequence: exhaust docker-hub apps first (Superset, Metabase, Discourse, Ghost, Matomo... ~300+ consented layouts, zero etiquette overhead); the crawl itself needs robots.txt + per-host budgets + Tranco-style URL seeding, AND a labeler upgrade first (cursor:pointer/tabindex/handler heuristics + label-density page filter + IGNORE-REGIONS for uncertain elements — excluded from loss, not taught as negatives; run on corpus apps too, a11y quality is a gradient not a binary per chris) — wild-web div-soup turns missing a11y into false-negative training labels that teach the model supervised blindness. Crawl shape: shallow-and-wide (few pages per site, trimmed grid ~6-8 variations, many sites) — cross-site diversity dwarfs per-page augmentation and it's the politest footprint anyway. Seed priority: .gov/.gov.uk (chris's call — 508/EN-301-549 mandates mean invested a11y, and gov sites are FORM-rich, rebalancing our starved textbox/checkbox/radio classes; seed from CISA's published dotgov-data + analytics.usa.gov, sample wide across agencies since design-system homogeneity decays per-page value)
- build.rs typed wrapper fns from accepted verb records
- COCO-format export for the detection corpus (RANKED FIRST of the hedge trio — expensive to retrofit once the format calcifies). The dataset is the durable asset: frameworks and inference regimes churn, an auto-labeled corpus survives every one of them. Harvest is already decoupled from training via portable on-disk artifacts; COCO is the marginal move that makes it ecosystem-legible — if PyTorch stays king the expensive asset (harvester + auto-labeling + corpus) ports for free and a PyTorch twin of the training loop is a weekend, not a rebuild. Bonus: "same detector, burn vs PyTorch, same data" is both a hedge and a killer blog post.
- Trait boundaries at the two ML seams: `Grounder` + `EffectJudge`, local burn models as impl #1. The executor and repair loop shouldn't know whether grounding came from burn, an ONNX import, or a cloud VLM if the economics flip. The ssim-must-lose gate already forces baseline and model through a common competing interface (effect-train's scored-slice protocol IS proto-EffectJudge) — the option is nearly free. CAVEAT (the warning that came with this): optionality STOPS at these boundaries — no backend-agnostic abstraction soup inside the training loop; goal #2's curriculum is served by bleeding on one concrete stack. Hedge the assets, commit the learning.
- Verb-schema guard: never leak burn types into verb records. Verbs-as-data with provenance means records don't care what grounded them — keep it that way and every accepted verb stays valid if the grounding regime changes in 2028. Cheap discipline now, enforce at schema review (4.1) and again at accept flow (5.2).
