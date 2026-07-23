//! Raw input dispatch: the primitive actions the harvester and the executor
//! share. Coordinates are CSS px (CDP dispatches pre-DPR).

use anyhow::Result;
use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchMouseEventParams, DispatchMouseEventType, InsertTextParams, MouseButton,
};

async fn press_release(page: &Page, x: f64, y: f64, button: MouseButton) -> Result<()> {
    for kind in [
        DispatchMouseEventType::MousePressed,
        DispatchMouseEventType::MouseReleased,
    ] {
        let mut params = DispatchMouseEventParams::new(kind, x, y);
        params.button = Some(button.clone());
        params.click_count = Some(1);
        page.execute(params).await?;
    }
    Ok(())
}

pub async fn click_at(page: &Page, x: f64, y: f64) -> Result<()> {
    press_release(page, x, y, MouseButton::Left).await
}

pub async fn right_click_at(page: &Page, x: f64, y: f64) -> Result<()> {
    press_release(page, x, y, MouseButton::Right).await
}

pub async fn hover_at(page: &Page, x: f64, y: f64) -> Result<()> {
    page.execute(DispatchMouseEventParams::new(
        DispatchMouseEventType::MouseMoved,
        x,
        y,
    ))
    .await?;
    Ok(())
}

/// Click to focus, then insert text through the CDP composition path —
/// reaches whatever is focused, no per-key event fabrication.
pub async fn type_at(page: &Page, x: f64, y: f64, text: &str) -> Result<()> {
    click_at(page, x, y).await?;
    page.execute(InsertTextParams::new(text)).await?;
    Ok(())
}
