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


