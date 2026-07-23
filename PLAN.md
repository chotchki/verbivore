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
- [ ] 2.2 - Anchor-free detector (FCOS/CenterNet-style) model definition
- [ ] 2.3 - Detection loss + target assignment
- [ ] 2.4 - Decode + NMS post-processing
- [ ] 2.5 - IoU/mAP eval harness
- [ ] 2.6 - Training loop: SupervisedTraining, checkpointing, metrics
- [ ] 2.7 - Cross-machine benchmark: M3 Max wgpu vs 2080 Ti CUDA f16, fixed epochs, record the decision
- [ ] 2.8 - Intent phrase to element ranking (classical text match for v1)
- [ ] 2.9 - Held-out app eval against the 80% top-1 gate

## Phase 3 - Effect validation
- [ ] 3.1 - Before/after pair capture around CDP actions
- [ ] 3.2 - CDP-signal labeler: DOM mutations, network activity, aria flips
- [ ] 3.3 - Negative pairs: dead-area clicks + no-action ambient-animation frames
- [ ] 3.4 - SSIM-threshold baseline, tuned + measured (the bar to beat)
- [ ] 3.5 - Spike: siamese distance vs change-region head, pick one
- [ ] 3.6 - Train pair model, eval vs 95% catch / 5% false-alarm gates and the baseline
- [ ] 3.7 - Sabotage harness: dead-pixel click rewiring + noisy animated fixture

## Phase 4 - Verb records + executor
- [ ] 4.1 - Verb schema: action enum, container-scoped intents, assertions, provenance block
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

- Canvas verbs: runtime grounding + coordinate actions for canvas content
- Public-site harvesting for corpus diversity
- build.rs typed wrapper fns from accepted verb records
