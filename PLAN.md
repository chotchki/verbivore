<!-- plan-bridge:phase-high-water=C -->
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
## Phase C - Affordance fusion (DOM as input prior)
- [x] C.1 - Affordance evidence harvest SHIPPED: AffordanceEvidence{bbox, channel, source} vectors in sidecars (serde-default back-compat pinned by test), affordance.rs two-pass collector — declarative JS sweep (cursor roots, native tags, tabindex, scrollables, draggable, onclick) + CDP DOMDebugger.getEventListeners with depth=-1 (whole subtree, ONE call; raw Runtime.evaluate for object refs since evaluate_expression serializes by value and explodes on window), listener types classified into pointer/keyboard/scroll, document/window + delegation-root (>90% viewport) listeners recorded AMBIENT with per-channel dedupe. Measured on gitea: 72 evidence/sample (22 pointer-listener — the 6.1 addEventListener blind spot closed, 1 ambient/channel), zero harvest slowdown. Browser test pins all evidence kinds incl. listener geometry + ambient
- [x] C.2 - Rasterizer + model plumbing SHIPPED: GroundingItem.prior (3 planes, AffordanceChannel order), rasterize_prior with heat = source_weight * min(1, A0/area) at A0 = 32x32 input px (listener/ambient 1.0, native 0.9, cursor 0.8, scrollable 0.7, tabindex 0.5), additive clamp-1.0; batcher emits [n, 6, 640, 640] so forward() and every call site stay untouched; stem 3->6 channels. Test pins full-weight at element size, 0.02 dilution on a 320x160 rect, ambient glow 0.005, exact zero off-evidence. Empty affordance = flat prior = canvas condition, by construction
- [x] C.3 - Per-channel prior dropout SHIPPED (augment.rs): train-loop tensor ops — 15% of batches go ALL-FLAT (canvas condition trained directly; independent drops would make it a 0.8% rarity), else each plane independently drops (0.20) / attenuates 0.2-0.8x (0.15) / flattens to mean (0.10) / box-blurs k=5-17 via avg_pool (0.10) / dilates k=5-9 via max_pool (0.10) / passes clean (0.35). LCG-deterministic off the training seed so B.1 bit-reproducibility survives; rgb never touched (pinned by test); eval paths never degrade
- [x] C.4 - Three-condition eval protocol SHIPPED ahead of any fused training: PriorCondition::{Full, Degraded{seed}, Flat} in eval.rs (evaluate_model_under; Flat zeroes item priors pre-batch, Degraded reuses the C.3 augmentation deterministically), train-eval prints heldout[flat] and heldout[degraded] lines with the full-prior line kept LAST for rotate.sh tail extraction. GATE stands: flat-prior fused >= pixels-only baseline (v8 rotation + B.1 bands), else the model copied the prior
- [ ] C.5 - Pointer-link relabel experiment: restore demoted links as labels now that the input carries the cursor bit; measure link AP with/without restoration — decides whether 8.2 demotion stays policy or becomes obsolete
  - [x] C.5.1 - Restore lever: --restore-pointer-links flag keeps demoted links as labels (Harvester field + CLI + farm test)
  - [x] C.5.2 - Treatment harvest: wordpress + mediawiki re-harvested with restoration into corpus-v9-relabel, split to by-app-v9r
  - [ ] C.5.3 - Treatment folds: wp-restored and mw-restored, seeds 42/43/44, train merges swap in the other treated app; control = v9 rotation folds
  - [ ] C.5.4 - Relabel verdict: link-AP + mAP seed-mean deltas vs control within B bands, full-vs-flat link AP shows whether the cursor bit carries it — demotion stays policy or dies
- [x] C.6 - Runtime rasterizer PROVEN, and the planned "cheaper approximation" dissolved: getEventListeners(depth=-1) made the harvest collector one protocol call, so harvest and inference share ONE code path (affordance::collect + rasterize_prior, now pub). Live gitea smoke: 73 evidence in 22ms, planes in 0ms — viable even per-step, never mind authoring/repair time. Ignore-gated test pins evidence>0, heat>0 and a 2s latency ceiling
- [x] C.7 - v9 fused corpus + 3-seed rotation: re-harvest all 17 apps with affordance sidecars (v8 predates C.1 so its priors are all-flat), rotate with seeds 42/43/44 per the B.2 credit rule; every fold-seed log now carries heldout[flat]/[degraded]/full — apply the C.4 gate (flat >= pixels-only v8 baseline within B bands) and judge fusion on seed-mean deltas

# Backlog (not yet phased)

- Click-centered pair crops for the effect model — MEASURED MOTIVATION (v6 retrain, 2026-07-24): the widget-rich corpus dropped heldout catch to 0.806 with an oracle ceiling of 0.861 — ~14% of Changed pairs are invisible at any threshold because a 14px checkbox tick is ~3px after the 1280x800 -> 256x160 downscale. The click coordinate is KNOWN at runtime: crop the before/after pair around it and the model gets full resolution exactly where change is expected (whole-page input stays for the no-click/control path)

- Density-gate calibration: revisit the flat threshold (0.3 since 7.1, and 8.4 made demoted links count as covered — remaining skips are genuinely-unlabeled custom widgets, grafana-class; maybe per-app percentile calibration)

- ~~SPA frontier discovery~~ DONE same day (chris's DOM-chain design): navigation targets keyed by root-to-target chain tokens (structure + text fragments — text separates same-menu siblings whose chains are structurally identical), farthest-first probing on fresh loads, landed urls join the normal guarded frontier; plus settle_render fixing the render race (grafana: 0 anchors at load, 32 after mount). Grafana: 1 page -> 14, saturation-stopped. Probe triggers only on href-dry pages (<3 admitted)

- Canvas verbs: runtime grounding + coordinate actions for canvas content (QuickSight accounts are dead; stand-ins: Grafana's uPlot panels are ALREADY canvas, Superset lands in phase 6 for ECharts depth)
- Public-INTERNET harvesting (the rung after phase 6's docker apps): robots.txt + per-host budgets + Tranco-style URL seeding; shallow-and-wide crawl shape (few pages per site, trimmed grid ~6-8 variations, many sites — cross-site diversity dwarfs per-page augmentation and it's the politest footprint anyway). Seed priority: .gov/.gov.uk (chris's call — 508/EN-301-549 mandates mean invested a11y, and gov sites are FORM-rich, rebalancing our starved textbox/checkbox/radio classes; seed from CISA's published dotgov-data + analytics.usa.gov, sample wide across agencies since design-system homogeneity decays per-page value)
- build.rs typed wrapper fns from accepted verb records
- COCO-format export for the detection corpus (RANKED FIRST of the hedge trio — expensive to retrofit once the format calcifies). The dataset is the durable asset: frameworks and inference regimes churn, an auto-labeled corpus survives every one of them. Harvest is already decoupled from training via portable on-disk artifacts; COCO is the marginal move that makes it ecosystem-legible — if PyTorch stays king the expensive asset (harvester + auto-labeling + corpus) ports for free and a PyTorch twin of the training loop is a weekend, not a rebuild. Bonus: "same detector, burn vs PyTorch, same data" is both a hedge and a killer blog post.
- Trait boundaries at the two ML seams: `Grounder` + `EffectJudge`, local burn models as impl #1. The executor and repair loop shouldn't know whether grounding came from burn, an ONNX import, or a cloud VLM if the economics flip. The ssim-must-lose gate already forces baseline and model through a common competing interface (effect-train's scored-slice protocol IS proto-EffectJudge) — the option is nearly free. CAVEAT (the warning that came with this): optionality STOPS at these boundaries — no backend-agnostic abstraction soup inside the training loop; goal #2's curriculum is served by bleeding on one concrete stack. Hedge the assets, commit the learning.
- Verb-schema guard: never leak burn types into verb records. Verbs-as-data with provenance means records don't care what grounded them — keep it that way and every accepted verb stays valid if the grounding regime changes in 2028. Cheap discipline now, enforce at schema review (4.1) and again at accept flow (5.2).
- **Deeper discovery for URL-starved apps: dokuwiki (63) and zengarden (83) sit under the fold floor because their url lists are short, not because the gate rejects pages — raise their discovery caps / add seed pages (zengarden has 20 mirrored designs but dedupe eats the grid; dokuwiki needs more seeded content pages)** — added 2026-07-25.
