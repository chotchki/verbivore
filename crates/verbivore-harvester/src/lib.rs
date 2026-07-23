//! Drives Chrome via chromiumoxide to harvest auto-labeled training data: the DOM
//! provides bounding boxes + roles at capture time so no human ever annotates.

pub mod effect_capture;
pub mod labels;

pub use effect_capture::{ActionPair, ActionSignals};

use anyhow::{Context, Result, anyhow};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::accessibility::GetFullAxTreeParams;
use chromiumoxide::cdp::browser_protocol::emulation::{
    MediaFeature, SetDeviceMetricsOverrideParams, SetEmulatedMediaParams,
};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::ScreenshotParams;
use futures::StreamExt;
use tokio::task::JoinHandle;
use verbivore_dataset::Dataset;

pub use labels::{Bbox, ElementLabel};

/// Default capture viewport. Labels are only valid against the screenshot they
/// were captured with — same viewport, DPR 1 — the pair is the dataset contract.
pub const VIEWPORT_W: i64 = 1280;
pub const VIEWPORT_H: i64 = 800;

/// One way to re-render a page. Every axis changes pixels without changing the
/// page's meaning — that's what makes the sweep free augmentation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Variation {
    /// CSS px. Below-breakpoint widths are deliberate: a 375px render is a
    /// DIFFERENT layout (collapsed nav), not a smaller copy of the desktop one.
    pub viewport: (i64, i64),
    /// CSS zoom on the root element. Labels come back in post-zoom coordinates
    /// (Chrome's standardized zoom scales geometry APIs) — a test pins this.
    pub zoom: f64,
    pub color_scheme: ColorScheme,
    /// Device pixel ratio. Screenshot pixels = viewport * dpr; labels are scaled
    /// to screenshot space at capture, and a test pins that against the png header.
    pub dpr: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorScheme {
    Light,
    Dark,
}

impl Default for Variation {
    fn default() -> Self {
        Self {
            viewport: (VIEWPORT_W, VIEWPORT_H),
            zoom: 1.0,
            color_scheme: ColorScheme::Light,
            dpr: 1.0,
        }
    }
}

impl Variation {
    /// Default sweep: 5 viewports (2 below common breakpoints) x 2 zooms x
    /// 2 schemes x 2 dprs = 40 renders per URL, ~1s each. Trim explicitly if
    /// that's too heavy for a sweep — never silently.
    pub fn default_grid() -> Vec<Variation> {
        let mut grid = Vec::new();
        for viewport in [(1280, 800), (1440, 900), (1024, 768), (768, 1024), (375, 812)] {
            for zoom in [1.0, 1.25] {
                for color_scheme in [ColorScheme::Light, ColorScheme::Dark] {
                    for dpr in [1.0, 2.0] {
                        grid.push(Variation {
                            viewport,
                            zoom,
                            color_scheme,
                            dpr,
                        });
                    }
                }
            }
        }
        grid
    }
}

/// What a sweep actually put in the dataset.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SweepOutcome {
    pub added: usize,
    pub deduped: usize,
}

/// One captured page: the raw inputs every downstream stage feeds on.
#[derive(Debug)]
pub struct PageSnapshot {
    pub screenshot_png: Vec<u8>,
    pub html: String,
    pub ax_nodes: Vec<AxSummary>,
    pub labels: Vec<ElementLabel>,
}

/// Accessibility node cut down to what grounding needs; the full AXNode carries
/// far more, none of it useful until selector snapping.
#[derive(Debug)]
pub struct AxSummary {
    pub role: Option<String>,
    pub name: Option<String>,
}

/// Owns a headless Chrome and its CDP event loop. One instance can snapshot many
/// pages; each snapshot opens and closes its own tab.
pub struct Harvester {
    browser: Browser,
    handler_task: JoinHandle<()>,
    /// Held for the browser's lifetime: chromiumoxide's default profile dir is a
    /// FIXED path, so concurrent launches trip Chrome's singleton lock without
    /// a unique dir per instance.
    _profile_dir: tempfile::TempDir,
}

impl Harvester {
    /// Launches headless Chrome from the system install (no download).
    pub async fn launch() -> Result<Self> {
        let profile_dir = tempfile::tempdir().context("creating chrome profile dir")?;
        let config = BrowserConfig::builder()
            .user_data_dir(profile_dir.path())
            .build()
            .map_err(|e| anyhow!("browser config: {e}"))?;
        let (browser, mut handler) = Browser::launch(config)
            .await
            .context("launching Chrome — is it installed?")?;
        // The handler stream IS the CDP connection; nobody polls it, nothing responds.
        let handler_task = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if event.is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            browser,
            handler_task,
            _profile_dir: profile_dir,
        })
    }

    /// Navigates a fresh tab and captures screenshot + HTML + a11y tree + labels.
    pub async fn snapshot(&self, url: &str) -> Result<PageSnapshot> {
        self.snapshot_with(url, &Variation::default()).await
    }

    /// Re-renders `url` under every variation, harvesting each straight into the
    /// dataset. Dedup is the dataset's job — a variation that renders identical
    /// pixels (a page ignoring color scheme, say) collapses to one sample.
    pub async fn harvest_variations(
        &self,
        dataset: &Dataset,
        url: &str,
        variations: &[Variation],
    ) -> Result<SweepOutcome> {
        let mut outcome = SweepOutcome::default();
        for variation in variations {
            let snap = self.snapshot_with(url, variation).await?;
            let added = dataset.add(
                url,
                variation.viewport.0,
                variation.viewport.1,
                variation.dpr,
                snap.labels,
                &snap.screenshot_png,
            )?;
            if added.deduped {
                outcome.deduped += 1;
            } else {
                outcome.added += 1;
            }
        }
        Ok(outcome)
    }

    pub async fn snapshot_with(&self, url: &str, variation: &Variation) -> Result<PageSnapshot> {
        let (vw, vh) = variation.viewport;
        let page = self.browser.new_page("about:blank").await?;
        // Metrics set BEFORE navigation so layout, quads and screenshot agree.
        page.execute(
            SetDeviceMetricsOverrideParams::builder()
                .width(vw)
                .height(vh)
                .device_scale_factor(variation.dpr)
                .mobile(false)
                .build()
                .map_err(|e| anyhow!("device metrics: {e}"))?,
        )
        .await?;
        page.execute(SetEmulatedMediaParams {
            media: None,
            features: Some(vec![MediaFeature {
                name: "prefers-color-scheme".into(),
                value: match variation.color_scheme {
                    ColorScheme::Light => "light".into(),
                    ColorScheme::Dark => "dark".into(),
                },
            }]),
        })
        .await?;
        page.goto(url).await?;
        page.wait_for_navigation().await?;
        if variation.zoom != 1.0 {
            page.evaluate(format!(
                "document.documentElement.style.zoom = '{}'",
                variation.zoom
            ))
            .await?;
        }

        let screenshot_png = page
            .screenshot(
                ScreenshotParams::builder()
                    .format(CaptureScreenshotFormat::Png)
                    .build(),
            )
            .await?;
        let html = page.content().await?;
        let ax = page.execute(GetFullAxTreeParams::default()).await?;
        let labels =
            labels::extract(&page, &ax.result.nodes, vw as f64, vh as f64, variation.dpr).await?;
        let ax_nodes = ax
            .result
            .nodes
            .iter()
            .filter(|n| !n.ignored)
            .map(|n| AxSummary {
                role: labels::ax_str(n.role.as_ref()),
                name: labels::ax_str(n.name.as_ref()),
            })
            .collect();
        page.close().await?;
        Ok(PageSnapshot {
            screenshot_png,
            html,
            ax_nodes,
            labels,
        })
    }

    /// Loads a page at the default rendering, clicks at viewport css px, and
    /// captures the before/after pair with its CDP ground-truth signals.
    pub async fn capture_action_pair(
        &self,
        url: &str,
        click_at: (f64, f64),
        settle_ms: u64,
    ) -> Result<ActionPair> {
        let page = self.browser.new_page("about:blank").await?;
        page.execute(
            SetDeviceMetricsOverrideParams::builder()
                .width(VIEWPORT_W)
                .height(VIEWPORT_H)
                .device_scale_factor(1.0)
                .mobile(false)
                .build()
                .map_err(|e| anyhow!("device metrics: {e}"))?,
        )
        .await?;
        page.goto(url).await?;
        page.wait_for_navigation().await?;
        effect_capture::arm(&page).await?;
        let pair = effect_capture::click_and_capture(&page, click_at.0, click_at.1, settle_ms).await?;
        page.close().await?;
        Ok(pair)
    }

    /// Shuts the browser down; dropping without this leaks a Chrome process.
    pub async fn close(mut self) -> Result<()> {
        self.browser.close().await?;
        let _ = self.browser.wait().await;
        self.handler_task.await.ok();
        Ok(())
    }
}
