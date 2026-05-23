//! Regime FSM: Occupancy-Driven Adaptive Physics Engine
//!
//! **Module**: Intelligent Regime Transition Logic (Auto-Transmission)
//! **Reference**: BM Folder REGIME_MACHINE.md (5-state occupancy FSM)
//! **Purpose**: Dynamically switch physics fidelity based on destruction event density
//!
//! # Strategic Design: The Auto-Transmission
//!
//! ## The Problem
//! Static 7-layer pipeline costs 9.4 μs on Ally X (120 Hz, 30.7 μs budget = 56.8% headroom).
//! But when destruction density spikes (100+ events), thermal throttling + frame drops occur.
//! **Solution**: Adaptive phase shedding in Regime 5 (4.6 μs, 84.9% headroom).
//!
//! ## The Hysteresis Asymmetry (Critical for Player Experience)
//! - **Up Transition** (Regime 4 → 5): Require 4 frames of high occupancy
//!   - Why: Avoid flickering physics during firefights where occupancy bounces 70-90 events
//!   - Effect: Stay in high-fidelity mode (Regime 1-4) longer (prefer quality)
//! - **Down Transition** (Regime 5 → 4): Require only 2 frames of low occupancy
//!   - Why: Quick recovery when thermal pressure drops (prefer thermal safety)
//!   - Effect: Return to full fidelity faster after destruction subsides
//!
//! ## Regime Definitions
//!
//! | Regime | Occupancy | Phases | Cost | Headroom | Use Case |
//! |--------|-----------|--------|------|----------|----------|
//! | 1 | 0-20 | Full L1-L7 | 9.4 μs | 56.8% | Menu, cutscene |
//! | 2 | 20-40 | Full L1-L7 | 9.4 μs | 56.8% | Light destruction |
//! | 3 | 40-60 | Full L1-L7 | 9.4 μs | 56.8% | Medium destruction |
//! | 4 | 60-80 | Full L1-L7 | 9.4 μs | 56.8% | Heavy destruction |
//! | 5 | 80+ | Shed L2, L5 | 4.6 μs | 84.9% | Thermal throttling |
//!
//! ## Phase Shedding (Option A)
//! ```ignore
//! Regime 1-4: L1 → L2(Lie) → L3(Diss) → L4(Back) → L5(Spectral) → L6(EMA) → L7(Hash)
//! Regime 5:   L1 → [SKIP]   → L3(Diss) → L4(Back) → [SKIP]       → L6(EMA) → L7(Hash)
//!
//! Savings:
//! - L2 (Lie-bracket): 3.5 μs saved
//! - L5 (Spectral):    1.2 μs saved
//! - Total:            4.7 μs → target 4.6 μs ✓
//! ```
//!
//! ## Anti-Cheat: Rule 4 (Regime Consistency)
//! Since regime is in hash: `H_t = FNV1A(Z ⊕ S ⊕ frame ⊕ protocol ⊕ regime)`
//! - An attacker cannot compute low-cost Regime 5 trajectory and fake it as Regime 1
//! - Different regimes = different valid hashes (expected)
//! - Validator checks: `computed_hash == state.h_t` with current `state.regime`
//! - If regime changed: verify at frame boundary (Rule 4 in Supervisor)

use std::collections::VecDeque;

/// Regime state (1-5, occupancy-driven)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Regime {
    One = 1,
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
}

impl Regime {
    /// Convert u8 to Regime (clamped to 1-5)
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => Regime::One,
            2 => Regime::Two,
            3 => Regime::Three,
            4 => Regime::Four,
            5 => Regime::Five,
            _ => {
                // Clamp out-of-range values
                if val < 1 {
                    Regime::One
                } else {
                    Regime::Five
                }
            }
        }
    }

    /// Convert to u8
    pub fn to_u8(&self) -> u8 {
        *self as u8
    }

    /// Human-readable name
    pub fn name(&self) -> &'static str {
        match self {
            Regime::One => "Regime 1 (Menu/Cutscene)",
            Regime::Two => "Regime 2 (Light)",
            Regime::Three => "Regime 3 (Medium)",
            Regime::Four => "Regime 4 (Heavy)",
            Regime::Five => "Regime 5 (Thermal)",
        }
    }

    /// Cost in microseconds for this regime
    pub fn cost_us(&self) -> f32 {
        match self {
            Regime::One | Regime::Two | Regime::Three | Regime::Four => 9.4,
            Regime::Five => 4.6,
        }
    }

    /// Headroom percentage for this regime
    pub fn headroom_pct(&self) -> f32 {
        match self {
            Regime::One | Regime::Two | Regime::Three | Regime::Four => 56.8,
            Regime::Five => 84.9,
        }
    }
}

/// Occupancy sample for history tracking
#[derive(Debug, Clone, Copy)]
struct OccupancySample {
    value: u32,
    #[allow(dead_code)]
    _frame_count: u64,
}

/// Regime FSM (Finite State Machine)
pub struct RegimeFsm {
    /// Current regime
    current_regime: Regime,
    /// Previous regime (for transition auditing)
    previous_regime: Regime,
    /// Frame count at last regime change
    regime_change_frame: u64,

    /// Occupancy history (rolling window)
    occupancy_history: VecDeque<OccupancySample>,
    /// Maximum history size
    history_size: usize,

    /// Hysteresis counters
    high_occupancy_frames: u32,  // Frames at high occupancy
    low_occupancy_frames: u32,   // Frames at low occupancy

    /// Thresholds for regime transitions
    /// Regime 1-4 ↔ 5 boundary
    up_threshold: u32,           // 80 events: transition up to Regime 5
    down_threshold: u32,         // 60 events: transition down to Regime 1-4

    /// Hysteresis requirements
    up_frames_required: u32,     // 4 frames at high occupancy before up transition
    down_frames_required: u32,   // 2 frames at low occupancy before down transition
}

impl RegimeFsm {
    /// Create new FSM (starts in Regime 1)
    pub fn new() -> Self {
        RegimeFsm {
            current_regime: Regime::One,
            previous_regime: Regime::One,
            regime_change_frame: 0,

            occupancy_history: VecDeque::with_capacity(10),
            history_size: 10,

            high_occupancy_frames: 0,
            low_occupancy_frames: 0,

            up_threshold: 80,
            down_threshold: 60,

            up_frames_required: 4,
            down_frames_required: 2,
        }
    }

    /// Get current regime
    pub fn current(&self) -> Regime {
        self.current_regime
    }

    /// Get previous regime (for transition auditing)
    pub fn previous(&self) -> Regime {
        self.previous_regime
    }

    /// Frame count at which current regime was established
    pub fn regime_change_frame(&self) -> u64 {
        self.regime_change_frame
    }

    /// Update FSM with occupancy from this frame
    ///
    /// This is the core state machine logic:
    /// 1. Track occupancy history
    /// 2. Update hysteresis counters
    /// 3. Determine regime transition (if any)
    /// 4. Update current_regime
    pub fn update(&mut self, occupancy: u32, frame_count: u64) {
        // Record occupancy sample
        self.occupancy_history.push_back(OccupancySample {
            value: occupancy,
            _frame_count: frame_count,
        });

        // Keep history bounded
        if self.occupancy_history.len() > self.history_size {
            self.occupancy_history.pop_front();
        }

        // Update hysteresis counters based on current occupancy
        if occupancy >= self.up_threshold {
            // High occupancy: count frames toward up transition
            self.high_occupancy_frames += 1;
            self.low_occupancy_frames = 0;  // Reset down counter
        } else if occupancy <= self.down_threshold {
            // Low occupancy: count frames toward down transition
            self.low_occupancy_frames += 1;
            self.high_occupancy_frames = 0;  // Reset up counter
        } else {
            // Middle zone: no transition (stay current)
            // But maintain current regime
        }

        // Determine regime based on hysteresis state
        let new_regime = self.determine_regime();

        // If regime changed, record transition
        if new_regime != self.current_regime {
            self.previous_regime = self.current_regime;
            self.current_regime = new_regime;
            self.regime_change_frame = frame_count;

            // Reset hysteresis counters
            self.high_occupancy_frames = 0;
            self.low_occupancy_frames = 0;
        }
    }

    /// Determine target regime based on hysteresis counters and thresholds
    fn determine_regime(&self) -> Regime {
        // Hysteresis asymmetry:
        // - Up transition (to Regime 5): Require 4 frames of high occupancy
        // - Down transition (from Regime 5): Require 2 frames of low occupancy

        match self.current_regime {
            Regime::One | Regime::Two | Regime::Three | Regime::Four => {
                // Currently in high-fidelity: check if should go to Regime 5
                if self.high_occupancy_frames >= self.up_frames_required {
                    Regime::Five
                } else {
                    self.current_regime
                }
            }
            Regime::Five => {
                // Currently in Regime 5: check if should return to Regime 1-4
                if self.low_occupancy_frames >= self.down_frames_required {
                    Regime::One  // Return to base regime
                } else {
                    Regime::Five
                }
            }
        }
    }

    /// Check if regime transition is valid (must be at frame boundary)
    ///
    /// This is Rule 4 (Supervisor Audit):
    /// Ensure regime only changes at frame boundaries, never mid-frame.
    pub fn is_transition_valid_at_frame(&self, current_frame: u64) -> bool {
        // Regime transition is valid if:
        // 1. No transition has occurred yet (previous == current), OR
        // 2. The transition happened at or before this frame (frame >= regime_change_frame)
        self.previous_regime == self.current_regime ||
        current_frame >= self.regime_change_frame
    }

    /// Get occupancy samples (for diagnostics)
    pub fn occupancy_history(&self) -> Vec<u32> {
        self.occupancy_history
            .iter()
            .map(|s| s.value)
            .collect()
    }

    /// Average occupancy over last N frames
    pub fn average_occupancy(&self, frames: usize) -> f32 {
        if self.occupancy_history.is_empty() {
            return 0.0;
        }

        let count = std::cmp::min(frames, self.occupancy_history.len());
        let sum: u32 = self
            .occupancy_history
            .iter()
            .rev()
            .take(count)
            .map(|s| s.value)
            .sum();

        sum as f32 / count as f32
    }

    /// Get FSM state for diagnostics
    pub fn diagnostic_state(&self) -> String {
        format!(
            "Regime: {} | High: {}/{} | Low: {}/{} | Avg Occupancy: {:.1}",
            self.current_regime.name(),
            self.high_occupancy_frames,
            self.up_frames_required,
            self.low_occupancy_frames,
            self.down_frames_required,
            self.average_occupancy(5)
        )
    }
}

impl Default for RegimeFsm {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Testing: Regime Transitions, Hysteresis, Phase Shedding
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regime_from_u8() {
        assert_eq!(Regime::from_u8(1), Regime::One);
        assert_eq!(Regime::from_u8(5), Regime::Five);
        assert_eq!(Regime::from_u8(0), Regime::One);     // Clamped low
        assert_eq!(Regime::from_u8(10), Regime::Five);   // Clamped high
    }

    #[test]
    fn test_regime_name() {
        assert_eq!(Regime::One.name(), "Regime 1 (Menu/Cutscene)");
        assert_eq!(Regime::Five.name(), "Regime 5 (Thermal)");
    }

    #[test]
    fn test_regime_cost_us() {
        assert_eq!(Regime::One.cost_us(), 9.4);
        assert_eq!(Regime::Five.cost_us(), 4.6);
    }

    #[test]
    fn test_regime_headroom() {
        assert_eq!(Regime::One.headroom_pct(), 56.8);
        assert_eq!(Regime::Five.headroom_pct(), 84.9);
    }

    #[test]
    fn test_fsm_initialization() {
        let fsm = RegimeFsm::new();
        assert_eq!(fsm.current(), Regime::One);
        assert_eq!(fsm.previous(), Regime::One);
    }

    #[test]
    fn test_fsm_no_transition_at_low_occupancy() {
        let mut fsm = RegimeFsm::new();

        // Low occupancy for many frames: should stay in Regime 1
        for frame in 0..10 {
            fsm.update(20, frame);
        }

        assert_eq!(fsm.current(), Regime::One);
    }

    #[test]
    fn test_fsm_transition_up_with_hysteresis() {
        let mut fsm = RegimeFsm::new();

        // High occupancy for 3 frames (not enough for transition)
        for frame in 0..3 {
            fsm.update(90, frame);
        }
        assert_eq!(fsm.current(), Regime::One, "Should not transition after 3 frames");

        // 4th frame: meets hysteresis requirement
        fsm.update(90, 3);
        assert_eq!(fsm.current(), Regime::Five, "Should transition to Regime 5 after 4 frames");
    }

    #[test]
    fn test_fsm_transition_up_requires_4_frames() {
        let mut fsm = RegimeFsm::new();

        // Exactly 4 frames required
        for frame in 0..4 {
            fsm.update(85, frame);
        }

        assert_eq!(fsm.current(), Regime::Five);
        assert_eq!(fsm.regime_change_frame(), 3);  // Changed on frame 3 (0-indexed)
    }

    #[test]
    fn test_fsm_transition_down_with_hysteresis() {
        let mut fsm = RegimeFsm::new();

        // Go to Regime 5
        for frame in 0..4 {
            fsm.update(85, frame);
        }
        assert_eq!(fsm.current(), Regime::Five);

        // Low occupancy for 1 frame (not enough)
        fsm.update(50, 4);
        assert_eq!(fsm.current(), Regime::Five, "Should not transition after 1 frame");

        // 2nd frame of low occupancy: meets hysteresis requirement
        fsm.update(50, 5);
        assert_eq!(fsm.current(), Regime::One, "Should transition to Regime 1 after 2 frames");
    }

    #[test]
    fn test_fsm_hysteresis_asymmetry() {
        let mut fsm = RegimeFsm::new();

        // Asymmetry: 4 frames up, 2 frames down
        // Go up quickly
        for frame in 0..4 {
            fsm.update(85, frame);
        }
        assert_eq!(fsm.current(), Regime::Five);

        // Go down faster
        fsm.update(50, 4);
        fsm.update(50, 5);
        assert_eq!(fsm.current(), Regime::One, "Down transition should require only 2 frames");
    }

    #[test]
    fn test_fsm_bouncing_occupancy_no_flicker() {
        let mut fsm = RegimeFsm::new();

        // Occupancy bounces around boundary (70-90 events)
        // This is the "firefight scenario" - should NOT flicker

        // Go to Regime 5
        for frame in 0..4 {
            fsm.update(85, frame);
        }
        assert_eq!(fsm.current(), Regime::Five);

        // Bounce low for 1 frame (not enough to transition)
        fsm.update(70, 4);
        assert_eq!(fsm.current(), Regime::Five, "Should stay in Regime 5 after 1 frame low");

        // Bounce back high
        fsm.update(85, 5);
        assert_eq!(fsm.current(), Regime::Five, "Should stay in Regime 5");

        // After 4+ frames of stability, stay in Regime 5
        fsm.update(85, 6);
        fsm.update(85, 7);
        assert_eq!(fsm.current(), Regime::Five, "Should remain stable in Regime 5");
    }

    #[test]
    fn test_fsm_occupancy_history() {
        let mut fsm = RegimeFsm::new();

        fsm.update(10, 0);
        fsm.update(20, 1);
        fsm.update(30, 2);

        let history = fsm.occupancy_history();
        assert_eq!(history, vec![10, 20, 30]);
    }

    #[test]
    fn test_fsm_average_occupancy() {
        let mut fsm = RegimeFsm::new();

        fsm.update(10, 0);
        fsm.update(20, 1);
        fsm.update(30, 2);

        let avg = fsm.average_occupancy(3);
        assert!((avg - 20.0).abs() < 0.1);  // (10+20+30)/3 = 20
    }

    #[test]
    fn test_fsm_regime_change_frame_tracking() {
        let mut fsm = RegimeFsm::new();

        assert_eq!(fsm.regime_change_frame(), 0);

        for frame in 0..4 {
            fsm.update(85, frame);
        }

        // Regime changed on frame 3
        assert_eq!(fsm.regime_change_frame(), 3);
    }

    #[test]
    fn test_fsm_previous_regime_tracking() {
        let mut fsm = RegimeFsm::new();

        assert_eq!(fsm.previous(), Regime::One);

        for frame in 0..4 {
            fsm.update(85, frame);
        }

        assert_eq!(fsm.previous(), Regime::One);
        assert_eq!(fsm.current(), Regime::Five);
    }

    #[test]
    fn test_fsm_middle_zone_stability() {
        let mut fsm = RegimeFsm::new();

        // Occupancy in middle zone (60-80): no hysteresis progress
        for frame in 0..10 {
            fsm.update(70, frame);
        }

        // Should stay in Regime 1 (never accumulates high or low frames)
        assert_eq!(fsm.current(), Regime::One);
    }

    #[test]
    fn test_fsm_diagnostic_state() {
        let mut fsm = RegimeFsm::new();
        for frame in 0..2 {
            fsm.update(85, frame);
        }

        let diag = fsm.diagnostic_state();
        assert!(diag.contains("Regime 1"));
        assert!(diag.contains("High: 2/4"));
    }

    #[test]
    fn test_fsm_transition_valid_at_frame() {
        let mut fsm = RegimeFsm::new();

        for frame in 0..4 {
            fsm.update(85, frame);
        }

        // Transition should be valid at the frame it occurred (frame 3)
        assert!(fsm.is_transition_valid_at_frame(3));

        // Valid at any frame after (no more transitions)
        assert!(fsm.is_transition_valid_at_frame(4));
    }

    #[test]
    fn test_fsm_long_session_stability() {
        let mut fsm = RegimeFsm::new();

        // Simulate 100 frames of varying occupancy
        for frame in 0..100 {
            let occupancy = if frame < 20 {
                10  // Regime 1
            } else if frame < 40 {
                50  // Middle
            } else if frame < 60 {
                85  // Regime 5
            } else {
                20  // Back to Regime 1
            };

            fsm.update(occupancy as u32, frame as u64);
        }

        // Should end in Regime 1 (low occupancy at end)
        assert_eq!(fsm.current(), Regime::One);
    }
}
