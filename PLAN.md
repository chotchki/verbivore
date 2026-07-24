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

## Phase 7 - Corpus v6 (upgraded discovery + design systems)

- [x] 7.1 - Design-system corpus apps, NINE new: bootstrap examples (official zip), USWDS/Materialize/Bulma/Fomantic/Pico kitchen sinks (official markup, pinned npm dists — fomantic picked BECAUSE its div-widgets stress ignore-regions at 0.35 coverage, pico as the native-element control group), W3C ARIA practices (107 example pages of definitionally correct widget labels), dokuwiki (mediawiki's blind-fold twin), css zen garden mirror (20 designs, one HTML — the variation thesis in its purest form, chris's callback). Label check confirms starved classes arriving: combobox/radio/slider/switch across the sinks. Density gate lowered 0.5 -> 0.3: ignore-regions now absorb what the gate used to block
- [ ] 7.2 - v6 discovery + harvest across all 13 apps with the upgraded crawler (chain probing, saturation, unified identity; page cap raised to 40 per chris — saturation owns stopping now)
- [ ] 7.3 - Rotation v6 (folds skip apps under 100 samples — a 31-sample heldout measures nothing) + effect-gate retrain with a FRESH threshold (v5's aged one slipped 0.8% past the FA gate)

# Backlog (not yet phased)

- Density-gate calibration: revisit the flat 0.5 threshold for grafana-class apps (65 variation skips in v5 — maybe per-app percentile calibration)

- ~~SPA frontier discovery~~ DONE same day (chris's DOM-chain design): navigation targets keyed by root-to-target chain tokens (structure + text fragments — text separates same-menu siblings whose chains are structurally identical), farthest-first probing on fresh loads, landed urls join the normal guarded frontier; plus settle_render fixing the render race (grafana: 0 anchors at load, 32 after mount). Grafana: 1 page -> 14, saturation-stopped. Probe triggers only on href-dry pages (<3 admitted)

- Canvas verbs: runtime grounding + coordinate actions for canvas content (QuickSight accounts are dead; stand-ins: Grafana's uPlot panels are ALREADY canvas, Superset lands in phase 6 for ECharts depth)
- Public-INTERNET harvesting (the rung after phase 6's docker apps): robots.txt + per-host budgets + Tranco-style URL seeding; shallow-and-wide crawl shape (few pages per site, trimmed grid ~6-8 variations, many sites — cross-site diversity dwarfs per-page augmentation and it's the politest footprint anyway). Seed priority: .gov/.gov.uk (chris's call — 508/EN-301-549 mandates mean invested a11y, and gov sites are FORM-rich, rebalancing our starved textbox/checkbox/radio classes; seed from CISA's published dotgov-data + analytics.usa.gov, sample wide across agencies since design-system homogeneity decays per-page value)
- build.rs typed wrapper fns from accepted verb records
- COCO-format export for the detection corpus (RANKED FIRST of the hedge trio — expensive to retrofit once the format calcifies). The dataset is the durable asset: frameworks and inference regimes churn, an auto-labeled corpus survives every one of them. Harvest is already decoupled from training via portable on-disk artifacts; COCO is the marginal move that makes it ecosystem-legible — if PyTorch stays king the expensive asset (harvester + auto-labeling + corpus) ports for free and a PyTorch twin of the training loop is a weekend, not a rebuild. Bonus: "same detector, burn vs PyTorch, same data" is both a hedge and a killer blog post.
- Trait boundaries at the two ML seams: `Grounder` + `EffectJudge`, local burn models as impl #1. The executor and repair loop shouldn't know whether grounding came from burn, an ONNX import, or a cloud VLM if the economics flip. The ssim-must-lose gate already forces baseline and model through a common competing interface (effect-train's scored-slice protocol IS proto-EffectJudge) — the option is nearly free. CAVEAT (the warning that came with this): optionality STOPS at these boundaries — no backend-agnostic abstraction soup inside the training loop; goal #2's curriculum is served by bleeding on one concrete stack. Hedge the assets, commit the learning.
- Verb-schema guard: never leak burn types into verb records. Verbs-as-data with provenance means records don't care what grounded them — keep it that way and every accepted verb stays valid if the grounding regime changes in 2028. Cheap discipline now, enforce at schema review (4.1) and again at accept flow (5.2).
