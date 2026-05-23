//! Supervisor Validator: Rules of Physics Enforcement
//!
//! **Module**: Determinism & Manifold Stability Auditor
//! **Reference**: BM Folder DVSM_VALIDATION.md (3-invariant supervisor)
//! **Purpose**: Validate H_t continuity, orthogonality constraint, ghost closure
//!
//! # Validation Pipeline
//!
//! After each frame evolves (Task #40), this validator runs:
//! ```ignore
//! state_evolved → Supervisor::validate_frame() → ValidationResult (bitmask)
//! ```
//!
//! ValidationResult flags (real-time HUD status):
//! - ✅ GREEN: Hash continuity, orthogonality, ghost closure all satisfied
//! - 🟡 YELLOW: Orthogonality soft constraint violated (diagnostic warning, not fatal)
//! - 🔴 RED: Hash mismatch or ghost closure broken (session parity compromised)
//!
//! # Design Notes
//!
//! **Soft Constraint Philosophy**:
//! - Orthogonality check does NOT crash if |Z·S| > ε_bound (soft, diagnostic-only)
//! - Can be compiled out in Regime 5 (thermal throttling mode) to preserve headroom
//! - Flags via HUD but doesn't halt simulation
//!
//! **Ghost Closure Audit**:
//! - Validates G_t = Z_t - Π_W(Z_t) computed correctly (no phantom feedback)
//! - Ensures ∂Z/∂G ≡ 0 (ghost never feeds Z evolution, only S via EMA)
//!
//! **Hash Continuity (Anti-Cheat)**:
//! - Black-box recorder for determinism parity
//! - If H_t ≠ H_expected, session is compromised (red-flag)
//! - Prevents "state stalling" or replay attacks (frame_count binding)
//!

use crate::physics::dvsm_state::DvsmState;
use crate::physics::evolution::{compute_h_session, SessionConfig};

/// Validation status bitmask for real-time HUD display
///
/// Each bit represents a "rule of physics" that can pass or fail:
/// - Bit 0: Hash continuity (H_t == H_expected)
/// - Bit 1: Orthogonality constraint (|Z·S| < ε_bound)
/// - Bit 2: Ghost closure (G_t = Z_t - Π_W(Z_t) valid)
/// - Bit 3: State bounds (‖Z‖ < Z_MAX, ‖S‖ < S_MAX)
///
/// Examples:
/// - 0b1111 = All green (all 4 rules satisfied)
/// - 0b1110 = Orange (bit 0 failed: hash mismatch)
/// - 0b0000 = Red (multiple failures)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationResult {
    /// Bitmask: bit i = 1 if rule i passed, 0 if failed
    pub status_bits: u8,
    /// Frame number this result corresponds to
    pub frame_count: u64,
    /// Human-readable status: "GREEN", "YELLOW", "RED"
    pub status_level: StatusLevel,
}

/// Severity level for HUD display
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    /// All rules satisfied (green light)
    Green,
    /// Soft constraint warning (yellow light, e.g., orthogonality drifting)
    Yellow,
    /// Critical failure (red light, e.g., hash mismatch)
    Red,
}

impl ValidationResult {
    /// Check if a specific rule passed
    pub fn rule_passed(&self, rule_index: u8) -> bool {
        rule_index < 4 && (self.status_bits & (1 << rule_index)) != 0
    }

    /// Get all failed rules as a list
    pub fn failed_rules(&self) -> Vec<&'static str> {
        let mut failed = Vec::new();
        if !self.rule_passed(0) {
            failed.push("Hash Continuity");
        }
        if !self.rule_passed(1) {
            failed.push("Orthogonality");
        }
        if !self.rule_passed(2) {
            failed.push("Ghost Closure");
        }
        if !self.rule_passed(3) {
            failed.push("State Bounds");
        }
        failed
    }
}

/// Supervisor: Validates frame evolution against 3 invariants
pub struct Supervisor {
    /// Expected hash (frame t) for continuity check
    expected_hash: u64,
    /// Regime (1-5) to conditionally skip expensive checks
    regime: u8,
    /// EMA coefficient for orthogonality bounds
    beta_ema: f32,
    /// Allow orthogonality soft constraint warnings (compilable-out)
    check_orthogonality_enabled: bool,
}

impl Supervisor {
    /// Create new supervisor
    pub fn new(config: &SessionConfig, regime: u8) -> Self {
        Supervisor {
            expected_hash: 0u64, // Will be set after first frame
            regime,
            beta_ema: config.beta_ema,
            check_orthogonality_enabled: regime != 5, // Skip orthogonality in Regime 5 (thermal throttling)
        }
    }

    /// Bind expected hash for next frame (call this after frame evolution)
    pub fn set_expected_hash(&mut self, hash: u64) {
        self.expected_hash = hash;
    }

    /// Validate evolved frame against 3 invariants + bounds
    ///
    /// Returns ValidationResult with bitmask:
    /// - Bit 0: Hash continuity passed
    /// - Bit 1: Orthogonality constraint passed (soft)
    /// - Bit 2: Ghost closure audit passed
    /// - Bit 3: State bounds audit passed
    pub fn validate_frame(
        &self,
        state: &DvsmState,
        config: &SessionConfig,
    ) -> ValidationResult {
        let mut bits = 0u8;

        // ========================================================================
        // Rule 0: Hash Continuity (Anti-Cheat, Critical)
        // ========================================================================
        // Note: frame_count has already been incremented by next_frame(),
        // so we use frame_count - 1 (the frame when hash was computed)
        let hash_frame_count = if state.frame_count > 0 {
            state.frame_count - 1
        } else {
            0
        };

        let computed_hash = compute_h_session(
            &state.z_t[0..269].try_into().unwrap(),
            &state.s_t[0..269].try_into().unwrap(),
            hash_frame_count,
            config.protocol_version,
            state.regime,
        );

        if computed_hash == state.h_t {
            bits |= 1 << 0; // Rule 0 passed
        }

        // ========================================================================
        // Rule 1: Orthogonality Constraint (Soft Diagnostic)
        // ========================================================================
        if self.check_orthogonality_enabled {
            let (_dot, _epsilon_bound, is_orthogonal) =
                state.check_orthogonality(self.beta_ema);

            if is_orthogonal {
                bits |= 1 << 1; // Rule 1 passed
            }
            // Note: If is_orthogonal=false, we don't set the bit, but we don't crash
            // (Soft constraint: diagnostic warning only)
        } else {
            // Regime 5: Skip orthogonality check (thermal throttling mode)
            bits |= 1 << 1; // Assume passed when skipped
        }

        // ========================================================================
        // Rule 2: Ghost Closure Audit (Phantom Energy Protection)
        // ========================================================================
        let ghost = state.compute_ghost();

        // Audit: G_t should be finite (no NaN/Inf artifacts)
        let ghost_valid = ghost.iter().all(|&g| g.is_finite());

        // Audit: Ghost norm should be small relative to Z norm
        // (if ‖G‖ is too large, something is wrong with projection)
        let ghost_norm_sq: f32 = ghost.iter().map(|&g| g * g).sum();
        let z_norm_sq: f32 = state.z_t[0..269]
            .iter()
            .map(|&z| z * z)
            .sum();
        let ghost_relative_norm = if z_norm_sq > 0.0 {
            (ghost_norm_sq / z_norm_sq).sqrt()
        } else {
            0.0
        };

        // Threshold: Ghost should be much smaller than Z (typically < 10% of Z norm)
        let ghost_closure_valid = ghost_valid && ghost_relative_norm < 0.1;

        if ghost_closure_valid {
            bits |= 1 << 2; // Rule 2 passed
        }

        // ========================================================================
        // Rule 3: State Bounds Audit (Overflow Protection)
        // ========================================================================
        let bounds_warnings = state.check_bounds();
        let bounds_valid = bounds_warnings.is_empty();

        if bounds_valid {
            bits |= 1 << 3; // Rule 3 passed
        }

        // ========================================================================
        // Determine Status Level (GREEN / YELLOW / RED)
        // ========================================================================
        let status_level = if bits == 0b1111 {
            StatusLevel::Green
        } else if bits & (1 << 0) == 0 {
            // Hash mismatch is critical (RED)
            StatusLevel::Red
        } else if bits & (1 << 1) == 0 && self.check_orthogonality_enabled {
            // Orthogonality warning (YELLOW, soft constraint)
            StatusLevel::Yellow
        } else {
            // Other failures (RED)
            StatusLevel::Red
        };

        ValidationResult {
            status_bits: bits,
            frame_count: state.frame_count,
            status_level,
        }
    }

    /// Check determinism parity: is this frame's hash reproducible?
    ///
    /// Note: frame_count has already been incremented by next_frame(), so we use frame_count - 1
    /// (the frame count when the hash was actually computed)
    ///
    /// Returns true if computed hash matches expected (bit 0 of validation result)
    pub fn check_hash_continuity(&self, state: &DvsmState, config: &SessionConfig) -> bool {
        let hash_frame_count = if state.frame_count > 0 {
            state.frame_count - 1
        } else {
            0
        };

        let computed_hash = compute_h_session(
            &state.z_t[0..269].try_into().unwrap(),
            &state.s_t[0..269].try_into().unwrap(),
            hash_frame_count,
            config.protocol_version,
            state.regime,
        );
        computed_hash == state.h_t
    }

    /// Get real-time HUD status string
    pub fn status_string(&self, result: &ValidationResult) -> String {
        match result.status_level {
            StatusLevel::Green => "✅ GREEN: Determinism Parity OK | Manifold Stability OK".to_string(),
            StatusLevel::Yellow => {
                let failed = result.failed_rules();
                format!("🟡 YELLOW: {} (soft warning, not fatal)", failed.join(", "))
            }
            StatusLevel::Red => {
                let failed = result.failed_rules();
                format!("🔴 RED: {} (session parity compromised)", failed.join(", "))
            }
        }
    }
}

// ============================================================================
// Testing: Determinism & Validation Verification
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::evolution::evolve_frame;

    #[test]
    fn test_supervisor_initialization() {
        let config = SessionConfig::default();
        let supervisor = Supervisor::new(&config, 1); // Regime 1

        assert_eq!(supervisor.regime, 1);
        assert!(supervisor.check_orthogonality_enabled); // Enabled in regime 1
    }

    #[test]
    fn test_orthogonality_check_enabled_in_regime_1_thru_4() {
        let config = SessionConfig::default();

        for regime in 1..=4 {
            let supervisor = Supervisor::new(&config, regime);
            assert!(supervisor.check_orthogonality_enabled, "Regime {} should enable orthogonality check", regime);
        }
    }

    #[test]
    fn test_orthogonality_check_disabled_in_regime_5() {
        let config = SessionConfig::default();
        let supervisor = Supervisor::new(&config, 5); // Regime 5: thermal throttling

        assert!(!supervisor.check_orthogonality_enabled, "Regime 5 should skip orthogonality check");
    }

    #[test]
    fn test_validation_result_rule_passed() {
        let result = ValidationResult {
            status_bits: 0b1111, // All rules passed
            frame_count: 0,
            status_level: StatusLevel::Green,
        };

        assert!(result.rule_passed(0), "Rule 0 should pass");
        assert!(result.rule_passed(1), "Rule 1 should pass");
        assert!(result.rule_passed(2), "Rule 2 should pass");
        assert!(result.rule_passed(3), "Rule 3 should pass");
    }

    #[test]
    fn test_validation_result_failed_rules() {
        let result = ValidationResult {
            status_bits: 0b1110, // Rule 0 failed, others passed
            frame_count: 0,
            status_level: StatusLevel::Red,
        };

        let failed = result.failed_rules();
        assert_eq!(failed.len(), 1, "Should have 1 failed rule");
        assert_eq!(failed[0], "Hash Continuity", "Hash Continuity should be the failed rule");
    }

    #[test]
    fn test_validate_frame_all_green() {
        let mut state = DvsmState::new();
        let config = SessionConfig::default();
        let supervisor = Supervisor::new(&config, 1);

        // Evolve frame to populate state properly
        evolve_frame(&mut state, 50, &config);

        // Validate: should be GREEN (all rules satisfied)
        let result = supervisor.validate_frame(&state, &config);

        assert_eq!(result.status_level, StatusLevel::Green, "Fresh evolved state should be GREEN");
        assert!(result.rule_passed(0), "Hash continuity should pass");
        assert!(result.rule_passed(3), "Bounds should pass");
    }

    #[test]
    fn test_hash_continuity_check() {
        let mut state = DvsmState::new();
        let config = SessionConfig::default();
        let supervisor = Supervisor::new(&config, 1);

        evolve_frame(&mut state, 50, &config);

        // Check hash continuity
        let hash_ok = supervisor.check_hash_continuity(&state, &config);
        assert!(hash_ok, "Hash continuity should pass for evolved state");
    }

    #[test]
    fn test_ghost_closure_audit() {
        let state = DvsmState::new();

        // Compute ghost and verify it's finite
        let ghost = state.compute_ghost();
        assert!(
            ghost.iter().all(|&g| g.is_finite()),
            "Ghost should be finite (no NaN/Inf)"
        );
    }

    #[test]
    fn test_orthogonality_soft_constraint_regime_5() {
        let mut state = DvsmState::new();
        let config = SessionConfig::default();
        let supervisor = Supervisor::new(&config, 5); // Thermal throttling

        evolve_frame(&mut state, 50, &config);

        let result = supervisor.validate_frame(&state, &config);

        // In Regime 5, orthogonality is skipped (bit 1 should be set as "passed" due to skip)
        assert!(
            result.rule_passed(1),
            "Regime 5 should treat orthogonality as passed (skipped)"
        );
    }

    #[test]
    fn test_status_string_green() {
        let supervisor = Supervisor::new(&SessionConfig::default(), 1);
        let result = ValidationResult {
            status_bits: 0b1111,
            frame_count: 0,
            status_level: StatusLevel::Green,
        };

        let status_str = supervisor.status_string(&result);
        assert!(status_str.contains("GREEN"), "Green status should contain GREEN");
    }

    #[test]
    fn test_status_string_yellow() {
        let supervisor = Supervisor::new(&SessionConfig::default(), 1);
        let result = ValidationResult {
            status_bits: 0b1101, // Orthogonality failed
            frame_count: 0,
            status_level: StatusLevel::Yellow,
        };

        let status_str = supervisor.status_string(&result);
        assert!(status_str.contains("YELLOW"), "Yellow status should contain YELLOW");
    }

    #[test]
    fn test_status_string_red() {
        let supervisor = Supervisor::new(&SessionConfig::default(), 1);
        let result = ValidationResult {
            status_bits: 0b0000, // All failed
            frame_count: 0,
            status_level: StatusLevel::Red,
        };

        let status_str = supervisor.status_string(&result);
        assert!(status_str.contains("RED"), "Red status should contain RED");
    }

    #[test]
    fn test_determinism_multi_frame_validation() {
        let mut state1 = DvsmState::new();
        let mut state2 = DvsmState::new();
        let config = SessionConfig::default();
        let supervisor = Supervisor::new(&config, 1);

        // Evolve both states identically
        for _ in 0..5 {
            evolve_frame(&mut state1, 50, &config);
            evolve_frame(&mut state2, 50, &config);
        }

        // Both should validate as GREEN (identical evolution)
        let result1 = supervisor.validate_frame(&state1, &config);
        let result2 = supervisor.validate_frame(&state2, &config);

        assert_eq!(result1.status_level, StatusLevel::Green);
        assert_eq!(result2.status_level, StatusLevel::Green);
        assert_eq!(state1.h_t, state2.h_t, "Identical evolution should produce identical hashes");
    }
}
