//! Drives Chrome via chromiumoxide to harvest auto-labeled training data: the DOM
//! provides bounding boxes + roles at capture time so no human ever annotates.

use anyhow::{Context, Result, anyhow};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::accessibility::{AxValue, GetFullAxTreeParams};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::ScreenshotParams;
use futures::StreamExt;
use tokio::task::JoinHandle;

/// One captured page: the raw inputs every downstream stage feeds on.
#[derive(Debug)]
pub struct PageSnapshot {
    pub screenshot_png: Vec<u8>,
    pub html: String,
    pub ax_nodes: Vec<AxSummary>,
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

    /// Navigates a fresh tab and captures screenshot + HTML + accessibility tree.
    pub async fn snapshot(&self, url: &str) -> Result<PageSnapshot> {
        let page = self.browser.new_page(url).await?;
        page.wait_for_navigation().await?;
        let screenshot_png = page
            .screenshot(
                ScreenshotParams::builder()
                    .format(CaptureScreenshotFormat::Png)
                    .full_page(true)
                    .build(),
            )
            .await?;
        let html = page.content().await?;
        let ax = page.execute(GetFullAxTreeParams::default()).await?;
        let ax_nodes = ax
            .result
            .nodes
            .iter()
            .filter(|n| !n.ignored)
            .map(|n| AxSummary {
                role: ax_str(n.role.as_ref()),
                name: ax_str(n.name.as_ref()),
            })
            .collect();
        page.close().await?;
        Ok(PageSnapshot {
            screenshot_png,
            html,
            ax_nodes,
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

fn ax_str(v: Option<&AxValue>) -> Option<String> {
    v.and_then(|v| v.value.as_ref())
        .and_then(|j| j.as_str().map(str::to_owned))
}
