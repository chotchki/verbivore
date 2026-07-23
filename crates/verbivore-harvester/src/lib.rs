//! Drives Chrome via chromiumoxide to harvest auto-labeled training data: the DOM
//! provides bounding boxes + roles at capture time so no human ever annotates.

pub mod labels;

use anyhow::{Context, Result, anyhow};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::accessibility::GetFullAxTreeParams;
use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::ScreenshotParams;
use futures::StreamExt;
use tokio::task::JoinHandle;

pub use labels::{Bbox, ElementLabel};

/// Fixed capture viewport. Labels are only valid against screenshots taken at
/// exactly this size with DPR 1 — the pair is the dataset contract.
pub const VIEWPORT_W: i64 = 1280;
pub const VIEWPORT_H: i64 = 800;

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
}

impl Harvester {
    /// Launches headless Chrome from the system install (no download).
    pub async fn launch() -> Result<Self> {
        let config = BrowserConfig::builder()
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
        })
    }

    /// Navigates a fresh tab and captures screenshot + HTML + a11y tree + labels.
    pub async fn snapshot(&self, url: &str) -> Result<PageSnapshot> {
        let page = self.browser.new_page("about:blank").await?;
        // DPR forced to 1 BEFORE navigation so layout and screenshot agree.
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

        let screenshot_png = page
            .screenshot(
                ScreenshotParams::builder()
                    .format(CaptureScreenshotFormat::Png)
                    .build(),
            )
            .await?;
        let html = page.content().await?;
        let ax = page.execute(GetFullAxTreeParams::default()).await?;
        let labels = labels::extract(
            &page,
            &ax.result.nodes,
            VIEWPORT_W as f64,
            VIEWPORT_H as f64,
        )
        .await?;
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

    /// Shuts the browser down; dropping without this leaks a Chrome process.
    pub async fn close(mut self) -> Result<()> {
        self.browser.close().await?;
        let _ = self.browser.wait().await;
        self.handler_task.await.ok();
        Ok(())
    }
}
