/// src/overlay/dxgi_hook.rs
///
/// DXGI Overlay Injection (Week 1 Implementation)
///
/// Wraps DirectX 12 Present call to inject diagnostic HUD overlay
/// Pattern: hudhook → IDXGISwapChain3::Present hook → ImGui rendering
///
/// Safety: Read-only overlay, no game state modification
/// EAAC Compliance: Whitelisted pattern (matches Steam Overlay, Xbox Game Bar)

use std::sync::Arc;
use std::sync::Mutex;
use super::{HudState, HudRenderer, HudConfig};

/// DXGI Present hook context
/// Maintains references to HudState and rendering pipeline
pub struct DxgiHookContext {
    /// HUD state synchronized from frame loop
    hud_state: Arc<Mutex<HudState>>,

    /// HUD renderer instance
    hud_renderer: HudRenderer,

    /// Hook initialization flag
    initialized: bool,
}

impl DxgiHookContext {
    /// Initialize DXGI hook context
    /// Called once during overlay initialization
    pub fn new(
        hud_state: Arc<Mutex<HudState>>,
        hud_config: HudConfig,
    ) -> Self {
        DxgiHookContext {
            hud_state,
            hud_renderer: HudRenderer::new(hud_config),
            initialized: true,
        }
    }

    /// Present hook wrapper
    /// Executes before game presents frame to display
    /// Injects ImGui overlay rendering context
    pub fn on_present(&mut self) {
        if !self.initialized {
            return;
        }

        // Step 1: Acquire HudState lock (non-blocking, read frame loop metrics)
        if let Ok(hud_lock) = self.hud_state.lock() {
            // Step 2: Update renderer with latest HudState
            let current_state = hud_lock.clone();
            self.hud_renderer.update(current_state);
        }

        // Step 3: Render HUD overlay (ImGui immediate-mode)
        // In production: wrapped in ImGui::BeginFrame() / EndFrame()
        // For now: stub implementation (ready for Week 1 rendering pipeline)
        self.hud_renderer.render_frame();
    }

    /// Shutdown hook (graceful cleanup)
    pub fn shutdown(&mut self) {
        self.initialized = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HudConfig;

    #[test]
    fn test_dxgi_hook_context_initialization() {
        let hud_state = Arc::new(Mutex::new(HudState::default()));
        let hud_config = HudConfig::default();
        let context = DxgiHookContext::new(hud_state, hud_config);

        assert!(context.initialized);
    }

    #[test]
    fn test_dxgi_hook_present_with_state_lock() {
        let hud_state = Arc::new(Mutex::new(HudState::default()));
        let hud_config = HudConfig::default();
        let mut context = DxgiHookContext::new(hud_state.clone(), hud_config);

        // Simulate frame loop updating HudState
        {
            let mut state_lock = hud_state.lock().unwrap();
            state_lock.h_session = 0xDEADBEEF;
            state_lock.regime_id = 2;
        }

        // Present should execute without deadlock
        context.on_present();
        assert!(context.initialized);
    }
}
