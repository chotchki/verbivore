## 2026-07-23

## Phase 1 - Harvester + corpus
- [x] 1.1 - Cargo workspace + crate layout (harvester, dataset, grounding, effect, executor, cli)
- [x] 1.2 - chromiumoxide spike: launch, navigate, screenshot, DOM + a11y tree extraction
- [x] 1.3 - Containerized corpus: Grafana + Gitea with seeded state
- [x] 1.4 - Label extraction: interactive elements to bbox + role + accessible name (DPR, scroll offsets, occlusion filtering)
- [x] 1.5 - On-disk dataset format, portable + versioned, with a stats tool
- [x] 1.6 - Re-render augmentation: themes, viewports, zoom, dpr + breakpoint widths


---

## 2026-07-23

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
- [x] 2.9 - Held-out app eval against the 80% top-1 gate
  - [x] 2.9.1 - Pipeline proven on real data: bias prior tames focal start (49.6 vs 19.7k), loss 4.5 @ 60 epochs; cross-app mAP 0.001 from a 4-layout corpus (honest baseline, matched-IoU 0.62)
  - [x] 2.9.2 - Corpus breadth: 6 apps / 48 layouts / 1232 samples — detector mAP curve 0.001 → 0.012 → 0.040, still superlinear (several hundred layouts projected for 0.3+, the docker-app rung covers it)
  - [x] 2.9.4 - Top-1 grounding gate on held-out gitea: 0.944 (3717/3937, avg 26 candidates) — PASSES the 80% bar; misses are name+role dupes, i.e. the container-scoping cases the spec predicted
  - [x] 2.9.3 - 6-fold rotation @30 epochs: mean mAP 0.026 +/- 0.026 (wordpress 0.078 best, mediawiki 0.000 worst) — spread equals mean, which app you hold out dominates; corpus breadth is THE lever


---

## 2026-07-23

## Phase 3 - Effect validation
- [x] 3.1 - Before/after pair capture around CDP actions
- [x] 3.2 - CDP-signal labeler: DOM mutations, network activity, aria flips
- [x] 3.3 - Negative pairs: dead-area clicks + no-action ambient-animation frames
- [x] 3.4 - SSIM baseline on 132 real pairs: catch 0.829 (0.926 on visible), FA 0.054, acc 0.879 — gates unmet, model has an honest job
- [x] 3.5 - Spike verdict: DIFF-STACK (1.000/0.000 on heldout, loss 0.13) beats siamese (0.857 — global embeddings wash out small local changes). Caveat: ssim also perfect on this 46-pair slice, too easy; 3.6 needs ambient-noise pages (grafana ?refresh=5s)
- [x] 3.6 - Train pair model, eval vs 95% catch / 5% false-alarm gates and the baseline. GATES PASS: diff-stack heldout 1.000/0.014 at train-frozen t=0.145 (oracle 1.000/0.000); ssim FAILS at every threshold (oracle ceiling 0.719/0.014 — subtle changes score BELOW its noise floor). 643 visible pairs / 76 urls. Checkpoint + sidecar in artifacts/effect-ckpt
  - [x] 3.6.1 - Noisy animated fixture (CSS noise + JS ticker + subtle-change buttons, URL variants) — doubles as 3.7's fixture source; 12 variants harvested clean over file://
  - [x] 3.6.2 - Split composition report + train-side ssim (threshold-transfer prerequisite; diagnosis: all ?refresh=5s pages hashed into train, heldout was all-quiet)
  - [x] 3.6.3 - effect-train bin: thresholds tuned on TRAIN and frozen for heldout, checkpoint + threshold sidecar for the phase-4 gate, heldout misclassification dump. FIXTURE VERDICT: ssim oracle ceiling now FAILS gates (0.971/0.065) — baseline officially beatable; diff-stack 1.000/0.065 at 60 epochs, unconverged
  - [x] 3.6.4 - Harvest fixture pairs into corpus, final gate run vs baseline (30 variants, stretched ambient window; run-36d.log in artifacts/)
  - [x] 3.6.5 - Labeler fix: per-node ambient suppression (count-subtraction aliases against periodic tickers — v17's 600ms period == settle window mislabeled dead clicks Changed) + per-url network suppression; regression test pins the aliasing scenario
  - [x] 3.6.6 - Purge corrupted fixture pairs, re-harvest all 30 variants with fixed labeler (second purge after the slow-ticker hole: ambient window now max(settle, 1500ms) — suppression lists don't need equal windows, longer is strictly better), retrain + gate verdict
- [x] 3.7 - Sabotage harness: dead-pixel click rewiring + noisy animated fixture. verbivore sabotage replays every element true-center vs rewired-to-dead-pixels through the signals-OR-visual gate, exits nonzero on a miss. 15/15 DETECTED across v3/v5/v23 (incl slow tickers); noop button correctly reads no-effect both ways — unvalidatable verbs are surfaced, not faked. Its first live catch was real: v3's 750ms ticker escaping the 600ms ambient window


---

## 2026-07-23

## Phase 4 - Verb records + executor
- [x] 4.1 - Verb schema: action enum (click, right-click, hover, type — recon-gen's diagram context menus need right-click; menus only exist post-interaction), container-scoped intents, assertions, provenance block. verbivore-verb crate: browser-free AND burn-free (the schema guard is structural — no dep to leak), one json per verb for reviewable repair diffs, per-rendering evidence variants (context miss = repair trigger), Action::Custom as the 4.3 hook, atomic saves
- [x] 4.2 - Generic executor: primitive actions over chromiumoxide. verbivore-executor: click/right-click/hover/type via shared harvester input primitives, per-step label re-extraction (menus only exist post-interaction), signals-half effect check per step, TYPED Breakage enum for the repair loop (first divergence aborts), status guard (candidates need review mode), assertions vs final a11y tree, run-verb CLI. 4/4 e2e vs fixture. Found+fixed: observer re-arm collided on const decls (install-once, reset-per-arm)
- [x] 4.3 - Custom-action registry (the quirk escape hatch). CustomRegistry: name -> boxed async fn with full page access; records carry only the NAME so they travel ahead of their impls — unregistered fails at execution as typed breakage, registered runs through the same per-step effect observation as primitives. Both paths e2e-tested
- [x] 4.4 - Effect gate: pair model as settle check + diagnostic bundle on failure. EffectJudge trait in executor (the hedge seam — no burn dep; BurnJudge adapter lives in the glue crate), asymmetric gate: Change = signals OR visual, NoChange = signals only (visual can't prove stillness on animated pages — live run broke on a 0.28 focus-ring score before the asymmetry). write_diagnostics bundle: run.json + per-step pngs. Live CLI run with the trained checkpoint judging every step: PASSED
- [x] 4.5 - Selector snapping: grounded element to role + accessible-name selector. snap_to_label (iou-gated, refuses dead space) in dataset crate, selector_for in verb crate (nth only for role+name twins, bbox-identity so clones find themselves). rank moved grounding -> dataset (pure, burn-free) so repair can use it; grounding re-exports
- [x] 4.6 - Repair loop: detect breakage, re-ground, patch record with reviewable diff. repair_verb in the (still burn-free) executor: run -> break_scene (labels + png captured at the failing step, THIS page not a fresh load) -> rank step intent over live a11y labels -> patch selector + evidence -> demote to Candidate -> verify-run. e2e: typo'd selector self-heals from intent, verified Passed. Dead elements stay Unrepairable — re-grounding can't resurrect what re-authoring must


---

## 2026-07-23

## Phase 5 - Generation + end-to-end
- [x] 5.1 - Crawler: walk a corpus app, propose task-level candidate verbs with scoped intents. verbivore-generator: container-aware page_map (ax parent_id walk to form/nav/dialog ancestors), pure propose() rules — NAMED forms become one task verb (type fields + submit click, container intents throughout), nav-container links become open-X, standalone buttons press-X; unnamed/whitespace-named forms and content-link soup skipped (candidate spam wastes review attention). Same-host BFS, logout/delete deny list, never clicks. Live: 9 candidates from 8 gitea pages
- [x] 5.2 - Candidate review + accept flow (status flip on the record). review_and_accept in executor: a candidate earns Accepted by RUNNING — Passed flips status, breakage rejects with the typed reason and status stays. accept-verb + list-verbs CLI
- [x] 5.3 - generate-verb CLI: intent phrase to accepted verb record. rank over live page_map -> selector snap -> candidate -> optional immediate review (--accept). Live on gitea: "the search button" grounded, accepted
- [x] 5.4 - End-to-end: generated verb runs deterministically, sabotage self-reports. full_loop e2e on the fixture: crawl -> review accepts press-toggle-details, REJECTS press-do-nothing as EffectSilence{step:0} (the self-report), accepted verb re-runs Passed. Real-app pass on gitea: crawl -> accept -> run-verb with the trained gate judging (visual 1.0 + navigated)
- [x] 5.5 - README + verb-format doc, sweep phase to archive. README: the loop diagram, measured-numbers section (detector's weakness stated plainly), try-it commands, crate map. docs/verb-format.md: the WHY per field — status lifecycle, three-address targets, asymmetric expect gate, provenance-as-string schema guard, variants-as-environment. SPEC end-to-end criterion marked MET

---

## 2026-07-24

## Phase 6 - Corpus expansion (docker apps)

MEASURED MOTIVATION (v4, 2026-07-23): +100 epochs and +18 deep-page samples replicated v3 (mAP 0.032 vs 0.040, loss 0.92->0.60 = overfit bending), same-app page mining dedupes to nothing; NEW APPS are the only remaining lever. Labeler upgrade comes FIRST — a11y quality is a gradient (per chris) and new apps with thinner a11y would turn missing labels into false-negative training data that teaches the model supervised blindness.

- [x] 6.1 - Labeler upgrade: interaction heuristics (cursor:pointer ROOTS only — the style inherits, tabindex, onclick, bare anchors) find what a11y missed -> IGNORE-REGIONS excluded from loss; SampleMeta.ignore (serde-default, old sidecars parse), dedupe now REFRESHES sidecars (labeling is labeler output, upgrades must reach captured samples), neg-focal masked at ignored cells (positives untouched — tested both directions). Known blind spot documented: addEventListener is invisible to a DOM scan, coverage is an upper bound
- [x] 6.2 - Label-density page filter: label_coverage on every snapshot, harvest_variations skips below 0.5 and counts low_density in the sweep outcome; div-soup fixture test pins the gate
- [x] 6.3 - New corpus apps in docker compose, pinned + headlessly seeded: Superset 4.0.2 (ECharts canvas stand-in; anonymous via Gamma-shaped Public role — anonymous-as-Admin 500s on ownership pages), Metabase v0.50 (wizard driven via API), Ghost 5.87 (dev-mode sqlite, self-seeding), Heimdall 2.6.1 (zero setup). SWAPS from the original list: Discourse skipped (compose complexity), Matomo -> Heimdall (install wizard can't be cleanly automated)
- [x] 6.4 - Crawler-driven harvest frontier: discover-urls reuses the 5.1 BFS + deny list, prints the visited frontier; smoke-tested on ghost (5 pages found). LOOP ARMOR added after chris's spiral question surfaced measured traps in the v5 discovery lists (gitea login?redirect_to=<every page>, wordpress replytocom, an assets/licenses.txt slot): FrontierGuard = per-family budget of 3 (family = path sans query), path-depth cap 8, non-html extension + machine-endpoint filters, 20x enqueue memory bound — shared by crawl and discover, unit-tested against the exact observed traps. FARTHEST-FIRST selection (chris's idea, hamming swapped for Jaccard over path segments + query keys — hamming is position-aligned and undefined across lengths): each pick maximizes min template-distance to visited, so a truncating budget spends across templates; measured on mediawiki, 6 distinct templates in 8 picks where BFS marched the sidebar. Also found+fixed mid-flight: mediawiki's root-owned sqlite 500ing (chown in seed.sh, idempotent)
- [x] 6.5 - Harvest sweep across new apps (variation grid) + merge: corpus-v5 = 1876 samples / 18133 labels across 8 apps (was 1250/6). Density gate live-fired 67 times, 65 on grafana (its custom widgets cover only 0-48% of interactive-looking surface — the a11y-gradient thesis measured on a real app)
- [x] 6.6 - Rotation re-run, 8 folds: mean cross-app mAP 0.059 (was 0.026 on 6 apps) — the diversity lever CONFIRMED, 2.3x from two more apps + cleaner labels. Best folds ghost 0.149, wordpress 0.115. Blind folds name their own fix: mediawiki 0.000 (the only wiki — needs a twin), heimdall 0.000 (31 samples, too small to hold out)
- [x] 6.7 - Effect-pair harvest on new apps + gate re-validation: diff-stack heldout 0.986/0.058 at frozen threshold on the grown corpus — catch holds, FA slips 0.8% past the 5% gate (oracle 0.986/0.047 passes; retune-after-corpus-growth is the lesson). ssim keeps failing everywhere (0.792/0.128, ceiling 0.764)

---

## 2026-07-24

## Phase 7 - Corpus v6 (upgraded discovery + design systems)

- [x] 7.1 - Design-system corpus apps, NINE new: bootstrap examples (official zip), USWDS/Materialize/Bulma/Fomantic/Pico kitchen sinks (official markup, pinned npm dists — fomantic picked BECAUSE its div-widgets stress ignore-regions at 0.35 coverage, pico as the native-element control group), W3C ARIA practices (107 example pages of definitionally correct widget labels), dokuwiki (mediawiki's blind-fold twin), css zen garden mirror (20 designs, one HTML — the variation thesis in its purest form, chris's callback). Label check confirms starved classes arriving: combobox/radio/slider/switch across the sinks. Density gate lowered 0.5 -> 0.3: ignore-regions now absorb what the gate used to block
- [x] 7.2 - v6 discovery + harvest across all 17 apps with the upgraded crawler (cap 40, saturation stopping): corpus-v6 = 2783 samples / 32926 labels / 5321 ignore-regions (+48%/+82% over v5). Grafana back in (314 samples, 1159 ignores — the lowered gate + mask working); starved classes real now (combobox 910, checkbox 370, radio 195, slider 69, switch 40; spinbutton still 2)
- [x] 7.3 - Rotation v6: mean 0.076 (v5 0.059, v3 0.026) — wordpress 0.191, bootstrap 0.148, ghost 0.118, gitea 0.105; dashboards flat ~0.05; mediawiki STILL 0.000 (all classes, even 1168 buttons — out-of-family pages collapse the ranking wholesale). PER-CLASS is the real story: on the wordpress fold button ap=0.400 and textbox ap=0.270 (controls ARE learning cross-app, the rebalance paid) but link ap=0.012 — and links are 63% of label mass, capping every aggregate. Effect retrain: FA fixed (0.047) but catch fell to 0.806, oracle 0.861 — tiny-widget changes are sub-resolution (click-centered crops backlogged with the numbers)

