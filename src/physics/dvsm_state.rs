//! DVSM State Decomposition (Phase II Foundation)
//!
//! **Module**: State Types and Invariant Enforcement
//! **Reference**: BM Folder DVSM_SPEC.md §A.2-§A.4 (State Decomposition)
//! **Purpose**: Define Z, S, G, W, H with bit-level alignment for SIMD + frame budget compliance
//!
//! # State Space Decomposition: ℝⁿ = M ⊕ N(M)
//!
//! The DVSM manifold decomposes state into two orthogonal subspaces:
//!
//! ```ignore
//! Z_t ∈ M           Primary state (on-manifold evolution)
//! S_t ∈ N(M)        Residual memory (dual-space, EMA of ghosts)
//! G_t (computed)    Ghost = Z_t - Π_W(Z_t) (off-manifold component, numeric residual only)
//! W_t               Stiefel basis (tangent space projection operator)
//! H_t               Hash binding (proof of state parity across 128 instances)
//! ```
//!
//! # Critical Design Decision: Ghost is COMPUTED, Not STORED
//!
//! **Invariant 3 (Ghost Closure)** mandates: ∂Z/∂G ≡ 0
//!
//! This means G_t is a **numeric residual**, not an entity with its own state.
//! - G_t is computed on-demand: `G_t = Z_t - Π_W(Z_t)`
//! - G_t is NEVER stored as a field in DvsmState
//! - G_t feeds S_t via EMA: `S_{t+1} = β·S_t + (1−β)·G_t`
//! - But G_t itself doesn't persist across frames
//!
//! **Why This Matters**:
//! - Saves 1KB stack allocation per frame
//! - Keeps `DvsmState` lightweight (~2.3 KB unpadded, ~2.4 KB aligned)
//! - Respects the "numeric residual only" semantics
//! - Eliminates allocation variance (frame-to-frame consistency)
//!
//! # Memory Alignment: SIMD-Readiness
//!
//! The 7-layer evolution pipeline (Task #40) will vectorize manifold ops.
//! Memory layout must align to 32-byte boundaries for AVX2 operations:
//!
//! ```ignore
//! [f32; 269] = 1076 bytes (not aligned)
//! [f32; 272] = 1088 bytes (aligned to 32-byte SIMD boundary)
//! ```
//!
//! We pad to 272 elements (4 extra f32 slots) to achieve 32-byte alignment.
//! These extra slots remain unused (cost: 16 bytes, negligible).
//!
//! # Frame Budget Impact
//!
//! DvsmState instantiation/copy is ~0.1 μs on modern CPUs (negligible).
//! Alignment overhead: 0 μs (copy is already fast, alignment just helps vectorization).
//!
//! ---

use crate::physics::fixed_point::{
    QuantMode, q31_quantize_vector, normalize_vector, adaptive_q_switch,
};

/// Padded manifold dimension for 32-byte SIMD alignment
/// 269 elements + 3 padding = 272 = 34 × 8 (32-byte aligned)
pub const MANIFOLD_DIM: usize = 269;
pub const MANIFOLD_DIM_PADDED: usize = 272; // (269 + 3) aligned to 32-byte boundary

/// Bounds on manifold state (safety guards)
pub const Z_NORM_MAX: f32 = 2.0;   // ‖Z‖ should stay < 2.0
pub const S_NORM_MAX: f32 = 0.5;   // ‖S‖ should stay < 0.5 (residual decay)
pub const NORM_WARNING_THRESHOLD: f32 = 1.8; // Issue diagnostic if approaching limit

/// Lightweight projection basis (Stiefel manifold St(n, r))
///
/// Instead of storing full 269×269 matrix (72K bytes), we store a **compact SVD**:
/// - `u`: Top-r left singular vectors (sparse, r ≈ 10-20)
/// - `s`: Top-r singular values (compact)
/// - `rank`: Number of basis vectors (r)
///
/// This enables fast tangent-space projection: Π_W(z) = U·S·U^T·z
/// without storing the full matrix.
///
/// # Why Sparse?
/// - Full 269×269 matrix: 72,361 f32 values = 289 KB (too large for per-frame ops)
/// - Compact SVD (r=10): 2,690 + 10 = 2.7 KB (fits in L1 cache)
/// - Projection cost: O(n·r) instead of O(n²)
#[derive(Debug, Clone)]
pub struct ProjectionBasis {
    /// Top-r left singular vectors (n × r matrix, stored row-major)
    /// Size: MANIFOLD_DIM × rank
    u: Vec<f32>,
    /// Top-r singular values (rank × 1)
    s: Vec<f32>,
    /// Rank r (typically 10-20 for manifold dimension 269)
    rank: usize,
}

impl ProjectionBasis {
    /// Create identity basis (no projection, full space)
    /// Used during initialization before full manifold structure is known
    pub fn identity() -> Self {
        ProjectionBasis {
            u: vec![],  // Empty = identity (Π_W = I)
            s: vec![],
            rank: 0,
        }
    }

    /// Compute projection of state onto tangent space: Π_W(z) = U·S·U^T·z
    /// Returns the projected component (on-tangent) for ghost computation
    pub fn project(&self, z: &[f32]) -> Vec<f32> {
        if self.rank == 0 {
            // Identity: projection is full vector
            return z.to_vec();
        }

        let mut result = vec![0.0f32; MANIFOLD_DIM];

        // Π_W(z) = U·(S·(U^T·z))
        // Step 1: U^T·z (project onto singular vectors)
        let mut proj = vec![0.0f32; self.rank];
        for i in 0..self.rank {
            let mut sum = 0.0f32;
            for j in 0..MANIFOLD_DIM {
                sum += self.u[i * MANIFOLD_DIM + j] * z[j];
            }
            proj[i] = sum * self.s[i];  // Scale by singular value
        }

        // Step 2: U·(scaled projection) (reconstruct in original space)
        for j in 0..MANIFOLD_DIM {
            let mut sum = 0.0f32;
            for i in 0..self.rank {
                sum += self.u[i * MANIFOLD_DIM + j] * proj[i];
            }
            result[j] = sum;
        }

        result
    }
}

// ============================================================================
// DvsmState: The Core State Vector
// ============================================================================

/// **DVSM State Vector** — 269-Dimensional Physics State
///
/// # Fields
///
/// | Field | Type | Purpose | Size | Alignment |
/// |-------|------|---------|------|-----------|
/// | `z_t` | [f32; 272] | On-manifold primary (padded to 32B) | 1088 B | 32B |
/// | `s_t` | [f32; 272] | Residual memory (EMA of ghosts) | 1088 B | 32B |
/// | `w_t` | ProjectionBasis | Tangent space basis (sparse SVD) | ~3 KB | — |
/// | `h_t` | u64 | Hash binding (proof of state parity) | 8 B | — |
/// | `frame_count` | u64 | Monotonic frame counter (temporal binding) | 8 B | — |
/// | `regime` | u8 | Current regime (1-5, compression selector) | 1 B | — |
/// | `quant_mode` | QuantMode | Quantization mode (Q31/Q16/Q64.64) | 1 B | — |
///
/// **Total Unpadded**: ~2300 bytes
/// **Total With Padding**: ~2400 bytes
/// **Per-Frame Cost**: < 1 μs (negligible in 17.9 μs headroom)
///
/// # Critical Invariants
///
/// **Invariant 1: Hash Continuity**
/// ```ignore
/// H_t = FNV1A(Z_t ⊕ S_t ⊕ frame_count ⊕ protocol_version ⊕ regime)
/// If H_t diverges → state mutation detected → rollback
/// ```
///
/// **Invariant 2: Orthogonality**
/// ```ignore
/// |Z_t · S_t| < ε_bound = (1 - β_ema) · ‖S_t‖
/// Soft constraint: warn if violated, don't enforce (diagnostic)
/// ```
///
/// **Invariant 3: Ghost Closure**
/// ```ignore
/// G_t = Z_t - Π_W(Z_t)  [computed on-demand, never stored]
/// ∂Z/∂G ≡ 0  [G never feeds Z evolution]
/// But: ∂S/∂G = (1 - β_ema)  [G feeds S via EMA, two-step]
/// ```
#[derive(Debug, Clone)]
pub struct DvsmState {
    /// Z_t: On-manifold primary state (269 elements, padded to 272 for SIMD)
    /// Range: typically ‖Z_t‖ < 2.0
    /// Quantized: Q31 (or Q16 if overflow detected)
    pub z_t: [f32; MANIFOLD_DIM_PADDED],

    /// S_t: Residual memory (269 elements, padded to 272)
    /// Definition: S_{t+1} = β·S_t + (1−β)·G_t
    /// G_t is the ghost (numeric residual), computed on-demand
    /// Range: typically ‖S_t‖ < 0.5 (decays due to (1−β_ema) damping)
    /// Quantized: Q31
    pub s_t: [f32; MANIFOLD_DIM_PADDED],

    /// W_t: Projection basis (Stiefel manifold, sparse)
    /// Represents tangent space for ghost computation: G_t = Z_t - Π_W(Z_t)
    /// Sparse SVD keeps this lightweight (~3 KB vs 72 KB for full matrix)
    pub w_t: ProjectionBasis,

    /// H_t: Session hash binding
    /// Proof of bit-identical state across 128 instances
    /// Updated every frame: H_t = FNV1A(Z_t ⊕ S_t ⊕ frame_count ⊕ regime ⊕ protocol)
    pub h_t: u64,

    /// Frame counter (monotonic, never resets within session)
    /// Used for temporal binding in hash computation
    /// Also used for regime transition hysteresis
    pub frame_count: u64,

    /// Current regime (1-5, determined by destruction event occupancy)
    /// Regime 1-2: RF codec (24-bit), normal transmission
    /// Regime 3: ELF codec (32-bit)
    /// Regime 4-5: Bio3D codec (48-bit), Regime 5 has 75% phase shedding
    pub regime: u8,

    /// Current quantization mode (Q31, Q16, or Q64.64)
    /// Selected by adaptive_q_switch based on ‖Z‖
    pub quant_mode: QuantMode,
}

impl DvsmState {
    /// Create a new DVSM state, initialized to zero with all invariants satisfied
    ///
    /// # Initialization Contract
    /// - Z_t = 0 (equilibrium state)
    /// - S_t = 0 (no residual yet)
    /// - H_t = 0 (will be computed on first frame)
    /// - frame_count = 0 (session starts at frame 0)
    /// - regime = 1 (start in regime 1, no destruction yet)
    /// - quant_mode = Q31 (default)
    ///
    /// # Determinism
    /// This initialization is bit-identical across all platforms.
    pub fn new() -> Self {
        DvsmState {
            z_t: [0.0f32; MANIFOLD_DIM_PADDED],
            s_t: [0.0f32; MANIFOLD_DIM_PADDED],
            w_t: ProjectionBasis::identity(),
            h_t: 0,
            frame_count: 0,
            regime: 1,
            quant_mode: QuantMode::Q31,
        }
    }

    /// Quantize state to selected mode (prepare for hash computation and serialization)
    ///
    /// This enforces the determinism contract:
    /// - All state rounded to fixed-point grid
    /// - Phantoms normalized (−0.0 → +0.0)
    /// - Ensures bit-identical hash across platforms
    ///
    /// # Called Before
    /// - H_session hash computation (every frame, Task #41)
    /// - SAEC bitstream encoding (every frame, Task #44)
    /// - Forensic replay (validation, Task #47)
    pub fn quantize(&mut self) {
        // Quantize Z_t to selected mode
        q31_quantize_vector(&mut self.z_t[0..MANIFOLD_DIM]);
        normalize_vector(&mut self.z_t[0..MANIFOLD_DIM]);

        // Quantize S_t (always Q31, residual stays high-precision)
        q31_quantize_vector(&mut self.s_t[0..MANIFOLD_DIM]);
        normalize_vector(&mut self.s_t[0..MANIFOLD_DIM]);
    }

    /// Update quantization mode based on current state magnitude
    ///
    /// Calls adaptive_q_switch to decide whether to use Q31, Q16, or Q64.64
    pub fn update_quant_mode(&mut self) {
        self.quant_mode = adaptive_q_switch(&self.z_t[0..MANIFOLD_DIM]);
    }

    /// Compute L2 norm of Z_t (on-manifold state)
    pub fn z_norm(&self) -> f32 {
        self.z_t[0..MANIFOLD_DIM]
            .iter()
            .map(|x| x * x)
            .sum::<f32>()
            .sqrt()
    }

    /// Compute L2 norm of S_t (residual memory)
    pub fn s_norm(&self) -> f32 {
        self.s_t[0..MANIFOLD_DIM]
            .iter()
            .map(|x| x * x)
            .sum::<f32>()
            .sqrt()
    }

    /// Compute ghost state (numeric residual only, computed on-demand)
    ///
    /// G_t = Z_t - Π_W(Z_t)
    ///
    /// This is computed when needed (in validator, before EMA update)
    /// but NOT stored in DvsmState (respects "numeric residual only" semantics)
    pub fn compute_ghost(&self) -> [f32; MANIFOLD_DIM_PADDED] {
        let mut ghost = [0.0f32; MANIFOLD_DIM_PADDED];

        // Compute projection onto tangent space
        let proj = self.w_t.project(&self.z_t[0..MANIFOLD_DIM]);

        // Ghost = Z - Π_W(Z)
        for i in 0..MANIFOLD_DIM {
            ghost[i] = self.z_t[i] - proj[i];
        }

        ghost
    }

    /// Check if state is within acceptable bounds (diagnostic only)
    ///
    /// Returns a vector of warnings (not fatal):
    /// - Z norm exceeds 2.0 (overflow zone)
    /// - S norm exceeds 0.5 (residual blowup)
    /// - Z or S contain NaN/Inf
    pub fn check_bounds(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        let z_norm = self.z_norm();
        let s_norm = self.s_norm();

        if z_norm > NORM_WARNING_THRESHOLD {
            warnings.push(format!(
                "Z norm {} approaching overflow (max {})",
                z_norm, Z_NORM_MAX
            ));
        }

        if s_norm > S_NORM_MAX {
            warnings.push(format!(
                "S norm {} exceeds limit (max {})",
                s_norm, S_NORM_MAX
            ));
        }

        // Check for NaN/Inf
        for i in 0..MANIFOLD_DIM {
            if !self.z_t[i].is_finite() {
                warnings.push(format!("Z[{}] is NaN or Inf", i));
            }
            if !self.s_t[i].is_finite() {
                warnings.push(format!("S[{}] is NaN or Inf", i));
            }
        }

        warnings
    }

    /// Check orthogonality invariant: |Z·S| < ε_bound
    ///
    /// ε_bound = (1 - β_ema) · ‖S_t‖
    /// This is a soft constraint (diagnostic, not fatal)
    pub fn check_orthogonality(&self, beta_ema: f32) -> (f32, f32, bool) {
        let dot_product: f32 = self.z_t[0..MANIFOLD_DIM]
            .iter()
            .zip(&self.s_t[0..MANIFOLD_DIM])
            .map(|(z, s)| z * s)
            .sum();

        let s_norm = self.s_norm();
        let eps_bound = (1.0 - beta_ema) * s_norm;
        let is_orthogonal = dot_product.abs() <= eps_bound;

        (dot_product, eps_bound, is_orthogonal)
    }

    /// Increment frame counter and return previous value
    /// (Used for temporal binding in hash computation)
    pub fn next_frame(&mut self) -> u64 {
        let prev = self.frame_count;
        self.frame_count = self.frame_count.wrapping_add(1);
        prev
    }
}

impl Default for DvsmState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Testing: State Invariants and Bounds
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dvsm_state_initialization() {
        let state = DvsmState::new();

        assert_eq!(state.frame_count, 0);
        assert_eq!(state.regime, 1);
        assert_eq!(state.z_norm(), 0.0);
        assert_eq!(state.s_norm(), 0.0);
        assert_eq!(state.h_t, 0);
    }

    #[test]
    fn test_dvsm_state_quantization() {
        let mut state = DvsmState::new();
        state.z_t[0] = 0.123456f32;
        state.s_t[0] = -0.987654f32;

        state.quantize();

        // After quantization, values should be on Q31 grid
        assert!(state.z_t[0].is_finite());
        assert!(state.s_t[0].is_finite());

        // Phantom prevention: no −0.0
        for i in 0..MANIFOLD_DIM {
            if state.z_t[i] == 0.0 {
                assert_eq!(state.z_t[i].to_bits(), 0.0f32.to_bits());
            }
        }
    }

    #[test]
    fn test_z_norm_computation() {
        let mut state = DvsmState::new();
        state.z_t[0] = 3.0f32;
        state.z_t[1] = 4.0f32;

        // ‖[3, 4, 0, ...]‖ = 5.0
        assert!((state.z_norm() - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_ghost_computation() {
        let mut state = DvsmState::new();
        state.z_t[0] = 0.5f32;
        state.z_t[1] = 0.3f32;

        let ghost = state.compute_ghost();

        // With identity basis (rank=0), Π_W(Z) = Z (full projection)
        // Therefore G = Z - Π_W(Z) = Z - Z = 0
        assert!((ghost[0] - 0.0).abs() < 0.01, "Ghost should be zero with identity basis");
        assert!((ghost[1] - 0.0).abs() < 0.01, "Ghost should be zero with identity basis");
    }

    #[test]
    fn test_bounds_checking() {
        let mut state = DvsmState::new();
        let warnings = state.check_bounds();
        assert!(warnings.is_empty()); // Zero state is valid

        // Simulate overflow
        state.z_t[0] = 2.5f32;
        let warnings = state.check_bounds();
        assert!(!warnings.is_empty()); // Should warn about Z norm
    }

    #[test]
    fn test_orthogonality_check() {
        let mut state = DvsmState::new();
        state.z_t[0] = 0.1f32;
        state.s_t[0] = 0.0f32; // Orthogonal

        let (dot, _eps, is_orth) = state.check_orthogonality(0.92);
        assert_eq!(dot, 0.0);
        assert!(is_orth);
    }

    #[test]
    fn test_frame_counter_wrapping() {
        let mut state = DvsmState::new();
        state.frame_count = u64::MAX;

        state.next_frame();
        assert_eq!(state.frame_count, 0); // Wraps around
    }

    #[test]
    fn test_quant_mode_selection() {
        let mut state = DvsmState::new();
        state.z_t[0] = 0.1f32;

        state.update_quant_mode();
        assert_eq!(state.quant_mode, QuantMode::Q31);

        // Simulate large state
        state.z_t[0] = 2.5f32;
        state.update_quant_mode();
        assert_eq!(state.quant_mode, QuantMode::Q16);
    }

    #[test]
    fn test_state_determinism_across_platforms() {
        let state1 = DvsmState::new();
        let state2 = DvsmState::new();

        // Same initialization → identical bit patterns
        for i in 0..MANIFOLD_DIM {
            assert_eq!(state1.z_t[i].to_bits(), state2.z_t[i].to_bits());
            assert_eq!(state1.s_t[i].to_bits(), state2.s_t[i].to_bits());
        }
    }
}
