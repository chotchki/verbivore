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

## Phase 3 - Effect validation
- [x] 3.1 - Before/after pair capture around CDP actions
- [x] 3.2 - CDP-signal labeler: DOM mutations, network activity, aria flips
- [x] 3.3 - Negative pairs: dead-area clicks + no-action ambient-animation frames
- [x] 3.4 - SSIM baseline on 132 real pairs: catch 0.829 (0.926 on visible), FA 0.054, acc 0.879 — gates unmet, model has an honest job
- [x] 3.5 - Spike verdict: DIFF-STACK (1.000/0.000 on heldout, loss 0.13) beats siamese (0.857 — global embeddings wash out small local changes). Caveat: ssim also perfect on this 46-pair slice, too easy; 3.6 needs ambient-noise pages (grafana ?refresh=5s)
- [ ] 3.6 - Train pair model, eval vs 95% catch / 5% false-alarm gates and the baseline
  - [x] 3.6.1 - Noisy animated fixture (CSS noise + JS ticker + subtle-change buttons, URL variants) — doubles as 3.7's fixture source; 12 variants harvested clean over file://
  - [x] 3.6.2 - Split composition report + train-side ssim (threshold-transfer prerequisite; diagnosis: all ?refresh=5s pages hashed into train, heldout was all-quiet)
  - [x] 3.6.3 - effect-train bin: thresholds tuned on TRAIN and frozen for heldout, checkpoint + threshold sidecar for the phase-4 gate, heldout misclassification dump. FIXTURE VERDICT: ssim oracle ceiling now FAILS gates (0.971/0.065) — baseline officially beatable; diff-stack 1.000/0.065 at 60 epochs, unconverged
  - [ ] 3.6.4 - Harvest fixture pairs into corpus, final gate run vs baseline
  - [x] 3.6.5 - Labeler fix: per-node ambient suppression (count-subtraction aliases against periodic tickers — v17's 600ms period == settle window mislabeled dead clicks Changed) + per-url network suppression; regression test pins the aliasing scenario
  - [ ] 3.6.6 - Purge corrupted fixture pairs, re-harvest all 30 variants with fixed labeler, retrain + gate verdict
- [ ] 3.7 - Sabotage harness: dead-pixel click rewiring + noisy animated fixture

## Phase 4 - Verb records + executor
- [ ] 4.1 - Verb schema: action enum (click, right-click, hover, type — recon-gen's diagram context menus need right-click; menus only exist post-interaction), container-scoped intents, assertions, provenance block
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
