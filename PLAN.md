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

- [x] 6.1 - Labeler upgrade: interaction heuristics (cursor:pointer ROOTS only — the style inherits, tabindex, onclick, bare anchors) find what a11y missed -> IGNORE-REGIONS excluded from loss; SampleMeta.ignore (serde-default, old sidecars parse), dedupe now REFRESHES sidecars (labeling is labeler output, upgrades must reach captured samples), neg-focal masked at ignored cells (positives untouched — tested both directions). Known blind spot documented: addEventListener is invisible to a DOM scan, coverage is an upper bound
- [x] 6.2 - Label-density page filter: label_coverage on every snapshot, harvest_variations skips below 0.5 and counts low_density in the sweep outcome; div-soup fixture test pins the gate
- [x] 6.3 - New corpus apps in docker compose, pinned + headlessly seeded: Superset 4.0.2 (ECharts canvas stand-in; anonymous via Gamma-shaped Public role — anonymous-as-Admin 500s on ownership pages), Metabase v0.50 (wizard driven via API), Ghost 5.87 (dev-mode sqlite, self-seeding), Heimdall 2.6.1 (zero setup). SWAPS from the original list: Discourse skipped (compose complexity), Matomo -> Heimdall (install wizard can't be cleanly automated)
- [x] 6.4 - Crawler-driven harvest frontier: discover-urls reuses the 5.1 BFS + deny list, prints the visited frontier; smoke-tested on ghost (5 pages found). LOOP ARMOR added after chris's spiral question surfaced measured traps in the v5 discovery lists (gitea login?redirect_to=<every page>, wordpress replytocom, an assets/licenses.txt slot): FrontierGuard = per-family budget of 3 (family = path sans query), path-depth cap 8, non-html extension + machine-endpoint filters, 20x enqueue memory bound — shared by crawl and discover, unit-tested against the exact observed traps. FARTHEST-FIRST selection (chris's idea, hamming swapped for Jaccard over path segments + query keys — hamming is position-aligned and undefined across lengths): each pick maximizes min template-distance to visited, so a truncating budget spends across templates; measured on mediawiki, 6 distinct templates in 8 picks where BFS marched the sidebar. Also found+fixed mid-flight: mediawiki's root-owned sqlite 500ing (chown in seed.sh, idempotent)
- [ ] 6.5 - Harvest sweep across new apps (variation grid) + merge, with the 6.2 report gating what enters training
- [ ] 6.6 - Retrain + leave-one-app-out rotation re-run: measure whether corpus diversity moves cross-app mAP (the experiment this phase exists for)
- [ ] 6.7 - Effect-pair harvest on new apps + gate re-validation (Superset canvas clicks are the visual-channel case signals can't see)

# Backlog (not yet phased)

- SPA frontier discovery: href-walking finds 1 page on grafana (nav is JS-driven, measured in the six-site crawler benchmark 2026-07-23) — historical url lists cover it today; the real fix is click-based nav discovery (crawl the nav by clicking, snapshot urls as they change) or app sitemap ingestion

- Canvas verbs: runtime grounding + coordinate actions for canvas content (QuickSight accounts are dead; stand-ins: Grafana's uPlot panels are ALREADY canvas, Superset lands in phase 6 for ECharts depth)
- Public-INTERNET harvesting (the rung after phase 6's docker apps): robots.txt + per-host budgets + Tranco-style URL seeding; shallow-and-wide crawl shape (few pages per site, trimmed grid ~6-8 variations, many sites — cross-site diversity dwarfs per-page augmentation and it's the politest footprint anyway). Seed priority: .gov/.gov.uk (chris's call — 508/EN-301-549 mandates mean invested a11y, and gov sites are FORM-rich, rebalancing our starved textbox/checkbox/radio classes; seed from CISA's published dotgov-data + analytics.usa.gov, sample wide across agencies since design-system homogeneity decays per-page value)
- build.rs typed wrapper fns from accepted verb records
- COCO-format export for the detection corpus (RANKED FIRST of the hedge trio — expensive to retrofit once the format calcifies). The dataset is the durable asset: frameworks and inference regimes churn, an auto-labeled corpus survives every one of them. Harvest is already decoupled from training via portable on-disk artifacts; COCO is the marginal move that makes it ecosystem-legible — if PyTorch stays king the expensive asset (harvester + auto-labeling + corpus) ports for free and a PyTorch twin of the training loop is a weekend, not a rebuild. Bonus: "same detector, burn vs PyTorch, same data" is both a hedge and a killer blog post.
- Trait boundaries at the two ML seams: `Grounder` + `EffectJudge`, local burn models as impl #1. The executor and repair loop shouldn't know whether grounding came from burn, an ONNX import, or a cloud VLM if the economics flip. The ssim-must-lose gate already forces baseline and model through a common competing interface (effect-train's scored-slice protocol IS proto-EffectJudge) — the option is nearly free. CAVEAT (the warning that came with this): optionality STOPS at these boundaries — no backend-agnostic abstraction soup inside the training loop; goal #2's curriculum is served by bleeding on one concrete stack. Hedge the assets, commit the learning.
- Verb-schema guard: never leak burn types into verb records. Verbs-as-data with provenance means records don't care what grounded them — keep it that way and every accepted verb stays valid if the grounding regime changes in 2028. Cheap discipline now, enforce at schema review (4.1) and again at accept flow (5.2).
