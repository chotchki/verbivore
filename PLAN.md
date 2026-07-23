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

## Phase 2 - Grounding detector
- [x] 2.1 - Burn scaffold: wgpu backend, dataset loader for the harvest format
- [x] 2.2 - Anchor-free detector (FCOS/CenterNet-style) model definition
- [x] 2.3 - Detection loss + target assignment
- [x] 2.4 - Decode + NMS post-processing
- [x] 2.5 - IoU/mAP eval harness
- [x] 2.6 - Training loop: hand-rolled epoch loop (SupervisedTraining's plumbing wasn't worth it), checkpointing, metrics
- [x] 2.7 - Cross-machine benchmark: M3 Max wgpu vs 2080 Ti CUDA f16, fixed epochs, record the decision
  - [x] 2.7.1 - Bench harness (train-bench bin, cuda feature) + metal side: 3.06s/epoch steady, 64 samples batch 8
  - [x] 2.7.2 - Ti/WSL2 result: 11.35s/epoch steady, loss parity to 4 digits. DECISION: train on the M3 Max, Ti retires (revisit only with f16 + release if scale demands)
- [x] 2.8 - Intent phrase to element ranking (classical text match for v1)
- [ ] 2.9 - Held-out app eval against the 80% top-1 gate
  - [x] 2.9.1 - Pipeline proven on real data: bias prior tames focal start (49.6 vs 19.7k), loss 4.5 @ 60 epochs; cross-app mAP 0.001 from a 4-layout corpus (honest baseline, matched-IoU 0.62)
  - [x] 2.9.2 - Corpus breadth: 6 apps / 48 layouts / 1232 samples — detector mAP curve 0.001 → 0.012 → 0.040, still superlinear (several hundred layouts projected for 0.3+, the docker-app rung covers it)
  - [x] 2.9.4 - Top-1 grounding gate on held-out gitea: 0.944 (3717/3937, avg 26 candidates) — PASSES the 80% bar; misses are name+role dupes, i.e. the container-scoping cases the spec predicted
  - [ ] 2.9.3 - Leave-one-app-out rotation (k-fold at app granularity): k trainings each holding out a different app, report mean mAP + spread — the honest error bar for unseen-app generalization

## Phase 3 - Effect validation
- [x] 3.1 - Before/after pair capture around CDP actions
- [x] 3.2 - CDP-signal labeler: DOM mutations, network activity, aria flips
- [x] 3.3 - Negative pairs: dead-area clicks + no-action ambient-animation frames
- [x] 3.4 - SSIM baseline on 132 real pairs: catch 0.829 (0.926 on visible), FA 0.054, acc 0.879 — gates unmet, model has an honest job
- [ ] 3.5 - Spike: siamese distance vs change-region head, pick one (train/eval on the VISIBLE subset — 8/76 Changed pairs paint nothing, they're the signal channel's job)
- [ ] 3.6 - Train pair model, eval vs 95% catch / 5% false-alarm gates and the baseline
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
- Public-site harvesting for corpus diversity — sequence: exhaust docker-hub apps first (Superset, Metabase, Discourse, Ghost, Matomo... ~300+ consented layouts, zero etiquette overhead); the crawl itself needs robots.txt + per-host budgets + Tranco-style URL seeding, AND a labeler upgrade first (cursor:pointer/tabindex/handler heuristics + label-density page filter + IGNORE-REGIONS for uncertain elements — excluded from loss, not taught as negatives; run on corpus apps too, a11y quality is a gradient not a binary per chris) — wild-web div-soup turns missing a11y into false-negative training labels that teach the model supervised blindness. Crawl shape: shallow-and-wide (few pages per site, trimmed grid ~6-8 variations, many sites) — cross-site diversity dwarfs per-page augmentation and it's the politest footprint anyway. Seed priority: .gov/.gov.uk (chris's call — 508/EN-301-549 mandates mean invested a11y, and gov sites are FORM-rich, rebalancing our starved textbox/checkbox/radio classes; seed from CISA's published dotgov-data + analytics.usa.gov, sample wide across agencies since design-system homogeneity decays per-page value)
- build.rs typed wrapper fns from accepted verb records
