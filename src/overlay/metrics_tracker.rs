/// src/overlay/metrics_tracker.rs
///
/// Metrics Collection & History (Circular Buffer)
/// - Frame time history (last 60 frames)
/// - H_session hash tracking
/// - Regime transition events
/// - L2 norm dissipation curve

use std::collections::VecDeque;

/// Metrics snapshot (per frame)
#[derive(Debug, Clone, Copy)]
pub struct MetricsSnapshot {
    pub frame_count: u32,
    pub frame_time_us: f32,
    pub h_session_hash: u64,
    pub regime: u8,
    pub l2_norm: f32,
}

/// Metrics tracker (history buffer)
/// Maintains dual arithmetic separation: Z (frame times) and S (residual EMA)
pub struct MetricsTracker {
    /// Frame time history (circular buffer, max 60 frames)
    /// Represents Z_t (forward evolution timeline)
    frame_times: VecDeque<f32>,
    max_history: usize,

    /// Current snapshot
    current: MetricsSnapshot,

    /// Hash history (detect changes)
    hash_history: VecDeque<(u32, u64)>, // (frame_count, hash)

    /// Regime transition log
    regime_transitions: VecDeque<(u32, u8)>, // (frame_count, new_regime)

    /// Dual residual state S_t (exponential moving average of residuals)
    /// Maintains orthogonality: forward evolution (Z) separate from residual tracking (S)
    residual_ema: f32,

    /// EMA smoothing factor α ∈ (0,1); default 0.2 for moderate smoothing
    ema_alpha: f32,

    /// Residual history (for observable closure verification)
    /// G_t = Z_t - Π_W(Z_t) where Π_W is windowed projection (baseline)
    residual_history: VecDeque<f32>,
}

impl MetricsTracker {
    /// Create tracker with history size
    pub fn new(max_history: usize) -> Self {
        MetricsTracker {
            frame_times: VecDeque::with_capacity(max_history),
            max_history,
            current: MetricsSnapshot {
                frame_count: 0,
                frame_time_us: 0.0,
                h_session_hash: 0,
                regime: 1,
                l2_norm: 0.0,
            },
            hash_history: VecDeque::with_capacity(20),
            regime_transitions: VecDeque::with_capacity(10),
            residual_ema: 0.0,
            ema_alpha: 0.2,  // Smoothing factor: balanced between responsiveness and stability
            residual_history: VecDeque::with_capacity(60),
        }
    }

    /// Record frame time
    pub fn push_frame_time(&mut self, frame_time_us: f32) {
        if self.frame_times.len() >= self.max_history {
            self.frame_times.pop_front();
        }
        self.frame_times.push_back(frame_time_us);
    }

    /// Record H_session hash
    pub fn push_hash(&mut self, frame_count: u32, hash: u64) {
        // Track if hash changed
        if let Some((_, last_hash)) = self.hash_history.back() {
            if hash != *last_hash {
                // Hash divergence detected
                eprintln!("⚠️  H_session divergence at frame {}", frame_count);
            }
        }

        if self.hash_history.len() >= 20 {
            self.hash_history.pop_front();
        }
        self.hash_history.push_back((frame_count, hash));
        self.current.h_session_hash = hash;
    }

    /// Record regime transition
    pub fn push_regime_transition(&mut self, frame_count: u32, new_regime: u8) {
        if new_regime != self.current.regime {
            self.regime_transitions.push_back((frame_count, new_regime));
            self.current.regime = new_regime;
            println!(
                "📊 Regime transition at frame {}: {} → {}",
                frame_count, self.current.regime, new_regime
            );
        }
    }

    /// Update L2 norm
    pub fn set_l2_norm(&mut self, l2_norm: f32) {
        self.current.l2_norm = l2_norm;
    }

    /// Get average frame time
    pub fn avg_frame_time(&self) -> f32 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.frame_times.iter().sum();
        sum / self.frame_times.len() as f32
    }

    /// Get P99 frame time
    pub fn p99_frame_time(&self) -> f32 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        let mut sorted: Vec<f32> = self.frame_times.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let index = (sorted.len() * 99) / 100;
        sorted[index]
    }

    /// Get frame time history
    pub fn frame_times(&self) -> &VecDeque<f32> {
        &self.frame_times
    }

    /// Get current snapshot
    pub fn current(&self) -> MetricsSnapshot {
        self.current
    }

    /// Get regime transition count
    pub fn regime_transition_count(&self) -> usize {
        self.regime_transitions.len()
    }

    /// Compute residual G_t = Z_t - Π_W(Z_t)
    /// Where Π_W(Z_t) is the windowed projection (running average baseline)
    /// Returns the residual value (deviation from baseline)
    pub fn compute_residual(&self) -> f32 {
        let baseline = self.avg_frame_time();
        self.current.frame_time_us - baseline
    }

    /// Update dual residual state S_t via exponential moving average
    /// S_{t+1} = α·G_t + (1-α)·S_t
    /// Maintains orthogonality: forward evolution (Z) separate from residual tracking (S)
    pub fn push_residual(&mut self) {
        let g_t = self.compute_residual();

        // EMA accumulation: S_{t+1} = α·G_t + (1-α)·S_t
        self.residual_ema = self.ema_alpha * g_t + (1.0 - self.ema_alpha) * self.residual_ema;

        // Store residual in history (capacity 60 frames)
        if self.residual_history.len() >= 60 {
            self.residual_history.pop_front();
        }
        self.residual_history.push_back(g_t);
    }

    /// Get current dual residual state S_t
    pub fn residual_state(&self) -> f32 {
        self.residual_ema
    }

    /// Get residual history (G_t sequence)
    pub fn residual_history(&self) -> &VecDeque<f32> {
        &self.residual_history
    }

    /// Set EMA smoothing factor (α)
    /// α ∈ (0,1): higher α = more responsive to recent residuals
    pub fn set_ema_alpha(&mut self, alpha: f32) {
        if alpha > 0.0 && alpha < 1.0 {
            self.ema_alpha = alpha;
        }
    }

    /// Clear history
    pub fn reset(&mut self) {
        self.frame_times.clear();
        self.hash_history.clear();
        self.regime_transitions.clear();
        self.residual_history.clear();
        self.residual_ema = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_tracker_frame_time() {
        let mut tracker = MetricsTracker::new(60);

        // Add frame times
        tracker.push_frame_time(6.5);
        tracker.push_frame_time(7.0);
        tracker.push_frame_time(6.8);

        assert_eq!(tracker.frame_times().len(), 3);
        assert!((tracker.avg_frame_time() - 6.7667).abs() < 0.01);
    }

    #[test]
    fn test_metrics_tracker_regime_transition() {
        let mut tracker = MetricsTracker::new(60);

        tracker.push_regime_transition(100, 1);
        tracker.push_regime_transition(200, 3); // Transition
        tracker.push_regime_transition(300, 3); // No change

        assert_eq!(tracker.regime_transition_count(), 1); // Only 1 transition recorded
    }

    #[test]
    fn test_metrics_tracker_hash_tracking() {
        let mut tracker = MetricsTracker::new(60);

        tracker.push_hash(10, 0xDEADBEEF);
        tracker.push_hash(11, 0xDEADBEEF); // Same hash
        tracker.push_hash(12, 0xCAFEBABE); // Different hash

        assert_eq!(tracker.current().h_session_hash, 0xCAFEBABE);
    }

    #[test]
    fn test_residual_computation() {
        let mut tracker = MetricsTracker::new(60);

        // Add frame times: 6.0, 7.0, 8.0 (average = 7.0)
        tracker.push_frame_time(6.0);
        tracker.push_frame_time(7.0);
        tracker.push_frame_time(8.0);

        // Current frame time set to 9.0
        tracker.current.frame_time_us = 9.0;

        // Residual: 9.0 - 7.0 = 2.0
        let residual = tracker.compute_residual();
        assert!((residual - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_ema_accumulation() {
        let mut tracker = MetricsTracker::new(60);
        tracker.set_ema_alpha(0.5);  // α = 0.5 for testing

        // Add baseline frame times (average will be 7.0)
        tracker.push_frame_time(6.0);
        tracker.push_frame_time(7.0);
        tracker.push_frame_time(8.0);

        // Frame 1: frame_time = 9.0, residual = 2.0
        tracker.current.frame_time_us = 9.0;
        tracker.push_residual();
        // S_1 = 0.5 * 2.0 + 0.5 * 0.0 = 1.0
        assert!((tracker.residual_state() - 1.0).abs() < 0.01);

        // Frame 2: frame_time = 5.0, residual = -2.0
        tracker.current.frame_time_us = 5.0;
        tracker.push_residual();
        // S_2 = 0.5 * (-2.0) + 0.5 * 1.0 = -0.5
        assert!((tracker.residual_state() - (-0.5)).abs() < 0.01);

        // Residual history should have 2 entries
        assert_eq!(tracker.residual_history().len(), 2);
    }

    #[test]
    fn test_residual_history_capacity() {
        let mut tracker = MetricsTracker::new(60);

        // Add baseline frame times
        tracker.push_frame_time(7.0);

        // Push 70 residuals (exceeds capacity of 60)
        for i in 0..70 {
            tracker.current.frame_time_us = 7.0 + (i as f32 * 0.1);
            tracker.push_residual();
        }

        // Should maintain capacity of 60
        assert_eq!(tracker.residual_history().len(), 60);
    }
}
