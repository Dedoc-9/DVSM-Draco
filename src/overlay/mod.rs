/// src/overlay/mod.rs
///
/// PHASE I.4b: Diagnostic HUD Overlay
/// DXGI Overlay (Safe-Path, Whitelisted Pattern)
///
/// **Design Philosophy**: Minimalist professional monitoring tool
/// (similar to Nvidia FrameView, GPU-Z, Steam Overlay)
/// - Non-intrusive position (top-right corner)
/// - Monospace font for technical credibility
/// - Subtle "Authorization Pending" watermark
/// - Real-time metrics without interpretation

pub mod hud_renderer;
pub mod metrics_tracker;
pub mod watermark;
pub mod dxgi_hook;

pub use hud_renderer::{HudRenderer, render_hud_frame};
pub use metrics_tracker::MetricsTracker;
pub use watermark::AuthorizationWatermark;
pub use dxgi_hook::DxgiHookContext;

/// HUD Configuration (minimalist design)
/// State variables for metric collection cadence, sparkline capacity, and watermark animation
#[derive(Debug, Clone)]
pub struct HudConfig {
    /// Polling rate (Hz) for metric collection cadence
    pub poll_rate_hz: u32,

    /// Sparkline history length (number of frame time samples)
    pub sparkline_len: usize,

    /// Watermark pulse frequency (Hz) for authorization status animation
    pub watermark_pulse_hz: f32,
}

impl Default for HudConfig {
    fn default() -> Self {
        HudConfig {
            poll_rate_hz: 120,           // Match BF6 frame rate
            sparkline_len: 60,           // 60-frame rolling history
            watermark_pulse_hz: 1.0,     // 1 Hz sine-wave fade
        }
    }
}

/// HUD State (frame-by-frame rendering)
/// State variables bound to observable operators: H_t (hash), regime (transmission mode), frame time statistics
#[derive(Debug, Clone)]
pub struct HudState {
    /// H_session: Current session hash (u64 bit-identical state signature)
    pub h_session: u64,

    /// regime_id: Current transmission regime (1-5)
    pub regime_id: u8,

    /// frame_time_avg_us: Averaged physics evolution time (f32, microseconds)
    pub frame_time_avg_us: f32,

    /// frame_time_p99_us: 99th percentile spike timing (f32, microseconds)
    pub frame_time_p99_us: f32,

    /// frame_history: Rolling buffer for sparkline (Vec<f32>, last N samples)
    pub frame_history: Vec<f32>,
}

impl Default for HudState {
    fn default() -> Self {
        HudState {
            h_session: 0xDEADBEEF_CAFE_BABE,
            regime_id: 1,
            frame_time_avg_us: 6.84,
            frame_time_p99_us: 8.12,
            frame_history: Vec::new(),
        }
    }
}
