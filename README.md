# verbivore

Vision-assisted verbs for browser testing: crawl a site, digest it into verbs.

Hand-written test verbs work (see [the approach this grew out of](https://hotchkiss.io/blog/instrumenting-playwright-the-gift-that-keeps-giving)) but carry two costs that never go away: selectors rot, and "did that click actually DO anything?" needs hand-built settle logic that fails silently when it's wrong. Verbivore aims small local vision models at exactly those two seams — trained on data the DOM labels for free — and keeps everything else deterministic. Pure Rust: [chromiumoxide](https://github.com/mattsse/chromiumoxide) drives the browser, [burn](https://github.com/tracel-ai/burn) trains and serves the models.

[SPEC.md](SPEC.md) owns the what and why; [PLAN.md](PLAN.md) tracks the build. Status: pre-alpha, harvesting works, nothing on crates.io yet.

## Layout

- `crates/verbivore` — the CLI
- `crates/verbivore-harvester` — drives Chrome, captures auto-labeled screenshots (viewport, zoom, color-scheme and dpr sweeps)
- `crates/verbivore-dataset` — on-disk dataset format; browser-free so training never links chromiumoxide
- `crates/verbivore-{grounding,effect,executor}` — element detection, effect validation, verb execution (not built yet)
- `corpus/` — dockerized apps to harvest: `cd corpus && docker compose up -d && ./seed.sh`

## Tests

`cargo test` needs a system Chrome install (it launches headless). Corpus-dependent tests are ignore-gated: bring the corpus up, then `cargo test -- --ignored`.

## License

MIT OR Apache-2.0.
