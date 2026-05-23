/// src/overlay/hud_renderer.rs
///
/// DXGI Overlay Rendering Engine (Minimalist Design)
///
/// Monospace, technical aesthetic (GPU-Z style)
/// - H_session hash (proof of 128-player sync)
/// - Regime indicator (phase shedding awareness)
/// - Frame time sparkline (thermal throttling detection)
/// - Authorization watermark (pending EA/DICE review)

use super::{HudConfig, HudState, AuthorizationWatermark};

/// Render HUD frame (pure rendering function)
/// State transformation: HudState → ImGui drawing commands
pub fn render_hud_frame(ui: &imgui::Ui, state: &HudState, frame_count: u32) {
    // Window configuration: top-right, 320x200 px
    ui.window("DRACO_OBSERVER")
        .position([1600.0 - 320.0 - 10.0, 10.0], imgui::Condition::FirstUseEver)
        .size([320.0, 200.0], imgui::Condition::FirstUseEver)
        .flags(
            imgui::WindowFlags::NO_TITLE_BAR
                | imgui::WindowFlags::NO_RESIZE
                | imgui::WindowFlags::NO_MOVE
                | imgui::WindowFlags::ALWAYS_AUTO_RESIZE,
        )
        .build(|| {
            // Title
            ui.text_colored([0.7, 0.7, 0.7, 1.0], "DRACO OBSERVER");
            ui.separator();

            // H_session hash (green hex display)
            ui.text_colored(
                [0.0, 1.0, 0.0, 1.0],
                format!("H_session: 0x{:016X}", state.h_session),
            );

            // Regime indicator (color-coded)
            let (regime_color, regime_name) = match state.regime_id {
                1 => ([0.0, 1.0, 0.0, 1.0], "Full Fidelity"),
                2 => ([0.0, 1.0, 1.0, 1.0], "High Fidelity"),
                3 => ([1.0, 1.0, 0.0, 1.0], "Balanced"),
                4 => ([1.0, 0.5, 0.0, 1.0], "Reduced"),
                5 => ([1.0, 0.0, 0.0, 1.0], "Phase Shedding"),
                _ => ([1.0, 1.0, 1.0, 1.0], "Unknown"),
            };
            ui.text_colored(
                regime_color,
                format!("Regime: {}/5 ({})", state.regime_id, regime_name),
            );

            // Frame time metrics (white text)
            ui.text_colored(
                [1.0, 1.0, 1.0, 1.0],
                format!(
                    "Avg: {:.2} μs | P99: {:.2} μs",
                    state.frame_time_avg_us, state.frame_time_p99_us
                ),
            );

            ui.separator();

            // Sparkline (60-frame history)
            if !state.frame_history.is_empty() {
                ui.plot_lines("##sparkline", &state.frame_history)
                    .scale_min(0.0)
                    .scale_max(30.7)  // Budget ceiling (120 Hz frame budget)
                    .graph_size([280.0, 40.0])
                    .build();
            }

            ui.separator();

            // Authorization watermark (pulsing gray text, bottom)
            let watermark = AuthorizationWatermark::pending();
            let watermark_color = watermark.rgba_for_frame(frame_count);
            ui.text_colored(
                [watermark_color.0, watermark_color.1, watermark_color.2, watermark_color.3],
                watermark.text(),
            );
        });
}

/// HUD Renderer (ImGui-based via hudhook)
pub struct HudRenderer {
    #[allow(dead_code)]
    config: HudConfig,
    state: HudState,
    frame_count: u32,
}

impl HudRenderer {
    /// Initialize renderer
    pub fn new(config: HudConfig) -> Self {
        HudRenderer {
            config,
            state: HudState::default(),
            frame_count: 0,
        }
    }

    /// Update state (per-frame)
    pub fn update(&mut self, new_state: HudState) {
        self.state = new_state;
        self.frame_count = self.frame_count.wrapping_add(1);
    }

    /// Render HUD frame via external ui context
    /// Called from hudhook Present hook with imgui::Ui instance
    pub fn render_frame_with_ui(&self, ui: &imgui::Ui) {
        render_hud_frame(ui, &self.state, self.frame_count);
    }

    /// Legacy render_frame (stub for compatibility)
    pub fn render_frame(&self) {
        // Placeholder: actual rendering requires imgui::Ui context from hudhook
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hud_config_defaults() {
        let config = HudConfig::default();
        assert_eq!(config.poll_rate_hz, 120);
        assert_eq!(config.sparkline_len, 60);
        assert_eq!(config.watermark_pulse_hz, 1.0);
    }

    #[test]
    fn test_hud_state_defaults() {
        let state = HudState::default();
        assert_eq!(state.h_session, 0xDEADBEEF_CAFE_BABE);
        assert_eq!(state.regime_id, 1);
        assert_eq!(state.frame_time_avg_us, 6.84);
        assert_eq!(state.frame_time_p99_us, 8.12);
        assert!(state.frame_history.is_empty());
    }

    #[test]
    fn test_hud_renderer_creation() {
        let renderer = HudRenderer::new(HudConfig::default());
        assert_eq!(renderer.config.poll_rate_hz, 120);
        assert_eq!(renderer.state.h_session, 0xDEADBEEF_CAFE_BABE);
    }

    #[test]
    fn test_hud_renderer_frame_count_increment() {
        let mut renderer = HudRenderer::new(HudConfig::default());
        assert_eq!(renderer.frame_count, 0);
        renderer.update(HudState::default());
        assert_eq!(renderer.frame_count, 1);
        renderer.update(HudState::default());
        assert_eq!(renderer.frame_count, 2);
    }
}
