/// src/lib.rs
///
/// Draco BF6 Edition Library
/// Phase I.4a: Shared Handle Reader + Framework
/// Phase I.4b: Diagnostic Overlay Infrastructure
/// Phase II: DVSM v3.3 Physics Kernel Integration (in progress)

pub mod interop;
pub mod overlay;
pub mod physics;

pub use interop::{
    SharedHandleReader,
    DestructionBitfield,
    TorsionSnapshot,
};

pub use overlay::{
    HudRenderer,
    MetricsTracker,
    AuthorizationWatermark,
    HudConfig,
    HudState,
    DxgiHookContext,
    render_hud_frame,
};

/// Diagnostic telemetry (for HUD overlay)
#[derive(Debug, Clone)]
pub struct DiagnosticTelemetry {
    pub frame_count: u32,
    pub destruction_events: u32,
    pub physics_regime: u8,
    pub h_session_hash: u64,
    pub l2_norm: f32,
    pub frame_time_us: u64,
    pub memory_used_mb: f32,
}

/// Observer mode configuration
#[derive(Debug, Clone)]
pub struct ObserverConfig {
    pub shared_handle_name: String,
    pub enable_overlay: bool,
    pub overlay_position: (i32, i32),
    pub polling_interval_us: u64,
    pub max_frame_budget_us: u64,
}

impl Default for ObserverConfig {
    fn default() -> Self {
        ObserverConfig {
            shared_handle_name: "BF6_Destruction_Global_0".to_string(),
            enable_overlay: true,
            overlay_position: (10, 10),
            polling_interval_us: 8_333, // 120 Hz
            max_frame_budget_us: 30_700, // Safe margin from 120Hz target
        }
    }
}

/// Global HudState for rendering thread access
use std::sync::OnceLock;

static HUD_STATE_GLOBAL: OnceLock<std::sync::Arc<std::sync::Mutex<HudState>>> = OnceLock::new();

/// Initialize global HUD state (called once at startup)
pub fn init_hud_state(state: std::sync::Arc<std::sync::Mutex<HudState>>) {
    let _ = HUD_STATE_GLOBAL.set(state);
}

/// Get reference to global HUD state
pub fn get_hud_state() -> std::sync::Arc<std::sync::Mutex<HudState>> {
    HUD_STATE_GLOBAL
        .get()
        .expect("HUD state not initialized")
        .clone()
}

/// Compute H_session hash binding
///
/// H_t = HASH(μ_t ⊕ Z_t ⊕ regime)
///
/// Where:
/// - μ_t: frame_count (temporal dimension, binned logical time)
/// - Z_t: z_manifold (269-dimensional physics state)
/// - regime: transmission regime (1-5, protocol version)
///
/// Returns: u64 hash token (bit-identical state signature for 128-player parity)
///
/// Safety: Bit-level normalization enforces zero floating-point phantom states.
/// Input -0.0 is converted to 0.0 before hashing to ensure determinism.
pub fn compute_h_session(
    manifold: &[f64; 269],
    frame_count: u64,
    regime: u8,
) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();

    // Hash frame count (temporal binding)
    frame_count.hash(&mut hasher);

    // Hash regime (protocol version)
    regime.hash(&mut hasher);

    // Hash manifold (physics state) with -0.0 normalization
    for &z in manifold.iter() {
        // Normalize -0.0 → 0.0 to prevent floating-point phantom divergence
        let z_normalized = if z == 0.0 {
            0.0  // Maps both 0.0 and -0.0 to positive zero
        } else {
            z
        };
        z_normalized.to_bits().hash(&mut hasher);
    }

    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observer_config_defaults() {
        let config = ObserverConfig::default();
        assert_eq!(config.polling_interval_us, 8_333);
        assert_eq!(config.max_frame_budget_us, 30_700);
    }

    #[test]
    fn test_compute_h_session_determinism() {
        // Two identical state vectors should produce identical hashes
        let mut manifold_a = [0.0f64; 269];
        let mut manifold_b = [0.0f64; 269];
        manifold_a[0] = 1.5;
        manifold_b[0] = 1.5;

        let hash_a = compute_h_session(&manifold_a, 100, 1);
        let hash_b = compute_h_session(&manifold_b, 100, 1);
        assert_eq!(hash_a, hash_b);
    }

    #[test]
    fn test_compute_h_session_divergence() {
        // Different frame counts should produce different hashes
        let manifold = [0.0f64; 269];

        let hash_frame_100 = compute_h_session(&manifold, 100, 1);
        let hash_frame_101 = compute_h_session(&manifold, 101, 1);
        assert_ne!(hash_frame_100, hash_frame_101);
    }

    #[test]
    fn test_compute_h_session_regime_sensitivity() {
        // Different regimes should produce different hashes
        let manifold = [0.0f64; 269];

        let hash_regime_1 = compute_h_session(&manifold, 100, 1);
        let hash_regime_5 = compute_h_session(&manifold, 100, 5);
        assert_ne!(hash_regime_1, hash_regime_5);
    }

    #[test]
    fn test_compute_h_session_negative_zero_normalization() {
        // -0.0 and 0.0 should produce identical hashes (phantom normalization)
        let manifold_pos = [0.0f64; 269];
        let mut manifold_neg = [0.0f64; 269];
        manifold_neg[0] = -0.0;

        let hash_pos = compute_h_session(&manifold_pos, 100, 1);
        let hash_neg = compute_h_session(&manifold_neg, 100, 1);
        assert_eq!(hash_pos, hash_neg);
    }
}
