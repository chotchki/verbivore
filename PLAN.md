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

## Phase 6 - Corpus expansion (docker apps)

MEASURED MOTIVATION (v4, 2026-07-23): +100 epochs and +18 deep-page samples replicated v3 (mAP 0.032 vs 0.040, loss 0.92->0.60 = overfit bending), same-app page mining dedupes to nothing; NEW APPS are the only remaining lever. Labeler upgrade comes FIRST — a11y quality is a gradient (per chris) and new apps with thinner a11y would turn missing labels into false-negative training data that teaches the model supervised blindness.

- [ ] 6.1 - Labeler upgrade: interaction heuristics (cursor:pointer, tabindex, click handlers) find what a11y missed -> IGNORE-REGIONS excluded from loss, not taught as negatives; dataset format grows an ignore field (backward-compatible), grounding loss masks ignored cells
- [ ] 6.2 - Label-density page filter + per-app quality report (heuristic-found vs a11y-labeled ratio; low-ratio pages skipped, the report names them)
- [ ] 6.3 - New corpus apps in docker compose, pinned + headlessly seeded: Superset (ECharts — also the canvas stand-in), Metabase, Ghost, Matomo (Discourse skipped: compose complexity is not worth one design system)
- [ ] 6.4 - Crawler-driven harvest frontier: discover-urls mode reuses the 5.1 BFS so per-app url lists stop being hand-curated
- [ ] 6.5 - Harvest sweep across new apps (variation grid) + merge, with the 6.2 report gating what enters training
- [ ] 6.6 - Retrain + leave-one-app-out rotation re-run: measure whether corpus diversity moves cross-app mAP (the experiment this phase exists for)
- [ ] 6.7 - Effect-pair harvest on new apps + gate re-validation (Superset canvas clicks are the visual-channel case signals can't see)

# Backlog (not yet phased)

- Canvas verbs: runtime grounding + coordinate actions for canvas content (QuickSight accounts are dead; stand-ins: Grafana's uPlot panels are ALREADY canvas, Superset lands in phase 6 for ECharts depth)
- Public-INTERNET harvesting (the rung after phase 6's docker apps): robots.txt + per-host budgets + Tranco-style URL seeding; shallow-and-wide crawl shape (few pages per site, trimmed grid ~6-8 variations, many sites — cross-site diversity dwarfs per-page augmentation and it's the politest footprint anyway). Seed priority: .gov/.gov.uk (chris's call — 508/EN-301-549 mandates mean invested a11y, and gov sites are FORM-rich, rebalancing our starved textbox/checkbox/radio classes; seed from CISA's published dotgov-data + analytics.usa.gov, sample wide across agencies since design-system homogeneity decays per-page value)
- build.rs typed wrapper fns from accepted verb records
- COCO-format export for the detection corpus (RANKED FIRST of the hedge trio — expensive to retrofit once the format calcifies). The dataset is the durable asset: frameworks and inference regimes churn, an auto-labeled corpus survives every one of them. Harvest is already decoupled from training via portable on-disk artifacts; COCO is the marginal move that makes it ecosystem-legible — if PyTorch stays king the expensive asset (harvester + auto-labeling + corpus) ports for free and a PyTorch twin of the training loop is a weekend, not a rebuild. Bonus: "same detector, burn vs PyTorch, same data" is both a hedge and a killer blog post.
- Trait boundaries at the two ML seams: `Grounder` + `EffectJudge`, local burn models as impl #1. The executor and repair loop shouldn't know whether grounding came from burn, an ONNX import, or a cloud VLM if the economics flip. The ssim-must-lose gate already forces baseline and model through a common competing interface (effect-train's scored-slice protocol IS proto-EffectJudge) — the option is nearly free. CAVEAT (the warning that came with this): optionality STOPS at these boundaries — no backend-agnostic abstraction soup inside the training loop; goal #2's curriculum is served by bleeding on one concrete stack. Hedge the assets, commit the learning.
- Verb-schema guard: never leak burn types into verb records. Verbs-as-data with provenance means records don't care what grounded them — keep it that way and every accepted verb stays valid if the grounding regime changes in 2028. Cheap discipline now, enforce at schema review (4.1) and again at accept flow (5.2).
