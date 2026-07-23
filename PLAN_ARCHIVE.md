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


