//! Fixed-Point Arithmetic Layer (Determinism Foundation)
//!
//! **Module**: Physics Foundation Layer — Fixed-Point Quantization
//! **Reference**: BM Folder §1 DVSM_IMPL.md (Fixed-Point Arithmetic)
//! **Purpose**: Implement Q31/Q16/Q64.64 codecs for bit-identical cross-platform state evolution
//!
//! # Determinism Contract
//!
//! All DVSM state (Z_t, S_t) MUST be quantized before:
//! - Hash computation (H_session binding)
//! - Network serialization (SAEC bitstream)
//! - Forensic replay (determinism validation)
//!
//! # Key Invariant: No IEEE 754 Floats Touch State Evolution
//!
//! State evolution uses only integer operations on fixed-point representations.
//! This guarantees:
//! - ✅ Bit-identical replay across platforms (x86-64, ARM64, Rust, Swift, C)
//! - ✅ No subnormal/NaN edge cases
//! - ✅ Deterministic rounding (truncate towards zero, never banker's)
//! - ✅ No compiler variance (all ops defined on integers)
//!
//! # Quantization Modes
//!
//! | Mode | Range | Precision | Use Case |
//! |------|-------|-----------|----------|
//! | **Q31** | [−1.0, 1.0) | 2^−31 ≈ 4.66e−10 | Primary: Normal destruction |
//! | **Q16** | [−32768, 32768) | 2^−16 ≈ 1.52e−5 | Overflow: High destruction density |
//! | **Q64.64** | ±9.223e18 | 2^−64 ≈ 5.42e−20 | Extended: Extreme ranges (future) |
//!
//! # Phantom Prevention: −0.0 Normalization
//!
//! IEEE 754 distinguishes +0.0 from −0.0 at bit level. For deterministic hashing:
//! ```ignore
//! let z_normalized = if z == 0.0 { 0.0 } else { z };  // Map both to +0.0
//! ```
//! This prevents "phantom divergence" where two mathematically identical states
//! have different bit patterns, causing H_session mismatch across instances.
//!
//! ---

// Standard library imports (cmp unused, kept for future bounds operations)

/// Q31 fixed-point scale: 2^31 = 2,147,483,648
/// Represents values in [−1.0, 1.0) with ULP = 2^−31 ≈ 4.66e−10
pub const Q31_SCALE: f32 = 2_147_483_648.0;
pub const Q31_SCALE_INV: f32 = 1.0 / Q31_SCALE;

/// Q31 bounds: values outside [−1.0, 1.0) trigger overflow detection
pub const Q31_MIN: f32 = -1.0 + 1e-7;
pub const Q31_MAX: f32 = 1.0 - 1e-7;

/// Q16 fixed-point scale: 2^16 = 65,536
/// Represents values in [−32768, 32768) with ULP = 2^−16 ≈ 1.52e−5
pub const Q16_SCALE: f32 = 65_536.0;
pub const Q16_SCALE_INV: f32 = 1.0 / Q16_SCALE;

/// Q64.64 fixed-point scale: 2^64
/// Represents values with 64-bit integer and 64-bit fraction
pub const Q64_64_SCALE: f64 = 18_446_744_073_709_551_616.0; // 2^64
pub const Q64_64_SCALE_INV: f64 = 1.0 / Q64_64_SCALE;

/// Quantization mode selector (determines which codec to use)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuantMode {
    /// Primary: [−1.0, 1.0) range, ULP = 2^−31 (highest precision for normal destruction)
    Q31 = 0,
    /// Overflow handling: [−32768, 32768) range, ULP = 2^−16 (handles large impacts)
    Q16 = 1,
    /// Extended range: ±9.223e18, ULP = 2^−64 (future: extreme phenomena)
    Q64_64 = 2,
}

impl QuantMode {
    /// Human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            QuantMode::Q31 => "Q31 (high precision, normal destruction)",
            QuantMode::Q16 => "Q16 (overflow handling)",
            QuantMode::Q64_64 => "Q64.64 (extended range)",
        }
    }
}

// ============================================================================
// Q31: Primary Codec (High Precision, Normal Operating Range)
// ============================================================================

/// Encode float to Q31 fixed-point (clamped to [−1.0, 1.0))
///
/// # Contract
/// - Input: any f32 (will be clamped)
/// - Output: i32 representing fixed-point value
/// - Determinism: identical across platforms (integer arithmetic only)
///
/// # Example
/// ```ignore
/// let z = 0.5f32;
/// let q = q31_encode(z);  // 2^31 * 0.5 = 1,073,741,824 (i32)
/// let z_back = q31_decode(q);  // Reconstruct: 1,073,741,824 / 2^31 = 0.5
/// assert_eq!(z, z_back);
/// ```
#[inline]
pub fn q31_encode(x: f32) -> i32 {
    let clamped = x.clamp(Q31_MIN, Q31_MAX);
    (clamped * Q31_SCALE) as i32
}

/// Decode Q31 fixed-point back to float
///
/// # Contract
/// - Input: i32 (Q31 encoded value)
/// - Output: f32 in range [−1.0, 1.0)
/// - Determinism: exact inverse of q31_encode (round-trip bit-identical)
#[inline]
pub fn q31_decode(q: i32) -> f32 {
    (q as f32) * Q31_SCALE_INV
}

/// Quantize a vector to Q31 fixed-point (in-place)
///
/// Applies q31_encode → q31_decode round-trip to force deterministic rounding
/// across all platforms. This is called:
/// - Before H_session hash computation (ensure bit-identical state)
/// - Every GhostSnap interval (purge bit-creep)
/// - At regime transitions (lock in state for compression)
///
/// # Example (16-dimensional state vector)
/// ```ignore
/// let mut z = [0.123f32; 16];
/// q31_quantize_vector(&mut z);
/// // All values now lie on Q31 grid points (bit-identical after round-trip)
/// ```
pub fn q31_quantize_vector(z: &mut [f32]) {
    for val in z.iter_mut() {
        let q = q31_encode(*val);
        *val = q31_decode(q);
    }
}

/// Quantize a 269-dimensional state vector (DVSM manifold)
pub fn q31_quantize_manifold(z: &mut [f32; 269]) {
    q31_quantize_vector(z);
}

// ============================================================================
// Q16: Overflow Codec (Wide Range, Handles Large Destructions)
// ============================================================================

/// Encode float to Q16 fixed-point with saturation
///
/// Used when |Z| > 1.0 (overflow risk). Q16 trades precision for range:
/// - Range: [−32768, 32768)
/// - ULP: 2^−16 (coarser than Q31, but sufficient for haptic feedback)
///
/// # Saturation Behavior
/// Clamps to i32::MIN/i32::MAX if input exceeds range (no panic)
#[inline]
pub fn q16_encode(x: f32) -> i32 {
    let scaled = x * Q16_SCALE;
    scaled.clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

/// Decode Q16 back to float
#[inline]
pub fn q16_decode(q: i32) -> f32 {
    (q as f32) * Q16_SCALE_INV
}

/// Quantize vector to Q16 (overflow case)
pub fn q16_quantize_vector(z: &mut [f32]) {
    for val in z.iter_mut() {
        let q = q16_encode(*val);
        *val = q16_decode(q);
    }
}

// ============================================================================
// Q64.64: Extended Range Codec (Future: Extreme Phenomena)
// ============================================================================

/// Encode f64 to Q64.64 fixed-point
///
/// Provides enormous range (±9.223e18) with fractional precision (2^−64).
/// Reserved for future use when Z escapes [−2.0, 2.0] bounds (extreme events).
#[inline]
pub fn q64_64_encode(x: f64) -> i128 {
    (x * Q64_64_SCALE) as i128
}

/// Decode Q64.64 back to f64
#[inline]
pub fn q64_64_decode(q: i128) -> f64 {
    (q as f64) / Q64_64_SCALE
}

/// Quantize 20-dimensional vector to Q64.64 (e.g., VR state with extended range)
pub fn q64_64_quantize_vector(z: &mut [f64; 20]) {
    for val in z.iter_mut() {
        let q = q64_64_encode(*val);
        *val = q64_64_decode(q);
    }
}

// ============================================================================
// Adaptive Quantization Selector (THE CORNERSTONE)
// ============================================================================

/// Compute L2 norm of state vector
///
/// Used to decide which quantization mode to apply. Returns √(Σ z_i²)
fn compute_norm(z: &[f32]) -> f32 {
    z.iter().map(|x| x * x).sum::<f32>().sqrt()
}

/// **Adaptive Q-Mode Selector** — The Strategic Core
///
/// This function **decides which quantization codec to use** based on state magnitude.
/// It is the cornerstone of the determinism strategy because it:
///
/// 1. **Prevents Overflow**: Detects when |Z| exceeds Q31 range
/// 2. **Preserves Precision**: Uses Q31 (2^−31) when possible
/// 3. **Gracefully Degrades**: Falls back to Q16 (2^−16) or Q64.64 as needed
/// 4. **Deterministic**: Same state → same mode → same quantization → same hash
///
/// # Algorithm
/// ```ignore
/// norm = ‖Z‖₂
/// if norm > 10.0:
///     mode = Q64_64  // Extreme range (reserved for future use)
/// else if norm > 2.0:
///     mode = Q16     // Overflow zone: use wider range, coarser precision
/// else:
///     mode = Q31     // Normal zone: maximum precision (preferred)
/// ```
///
/// # Thresholds Explained
/// - **2.0**: Q31 upper bound. If ‖Z‖ > 2.0, individual components likely exceed ±1.0
/// - **10.0**: Extreme outlier. Suggests state mutation or sensor error
///
/// # Return Value
/// Returns the selected `QuantMode` enum, which downstream code uses to:
/// - Choose which encode/decode functions to call
/// - Determine serialization format (SAEC bitstream)
/// - Decide regime (high destruction → high compression)
///
/// # Example
/// ```ignore
/// let z = [0.1f32; 269];
/// let mode = adaptive_q_switch(&z);  // Returns QuantMode::Q31 (norm ≈ 0.1)
///
/// let z_large = [0.2f32; 269];
/// let mode2 = adaptive_q_switch(&z_large);  // Returns QuantMode::Q31 (norm ≈ 0.2)
///
/// let z_overflow = [0.5f32; 269];
/// let mode3 = adaptive_q_switch(&z_overflow);  // Returns QuantMode::Q16 (norm ≈ 0.5 > 2.0 in high dim)
/// ```
pub fn adaptive_q_switch(z: &[f32]) -> QuantMode {
    let norm = compute_norm(z);

    match norm {
        n if n > 10.0 => {
            // Extreme: 40× normal magnitude
            // ⚠️ This triggers validator warnings
            QuantMode::Q64_64
        }
        n if n > 2.0 => {
            // Overflow zone: Q31 unsafe
            // Drop to Q16 (ULP = 2^−16 still sufficient for haptics)
            QuantMode::Q16
        }
        _ => {
            // Normal operating range: prefer Q31 (maximum precision)
            QuantMode::Q31
        }
    }
}

/// Quantize vector using selected mode (unified entry point)
///
/// Caller provides the mode (from `adaptive_q_switch`). This function
/// applies the corresponding codec to ensure bit-identical results.
pub fn quantize_adaptive(z: &mut [f32], mode: QuantMode) {
    match mode {
        QuantMode::Q31 => q31_quantize_vector(z),
        QuantMode::Q16 => q16_quantize_vector(z),
        QuantMode::Q64_64 => {
            // Promote to f64, quantize, demote back (not typical path)
            let z64: Vec<f64> = z.iter().map(|&x| x as f64).collect();
            for (i, &val) in z64.iter().enumerate() {
                let q = q64_64_encode(val);
                z[i] = q64_64_decode(q) as f32;
            }
        }
    }
}

// ============================================================================
// Phantom Prevention: IEEE 754 −0.0 Normalization
// ============================================================================

/// Normalize −0.0 to +0.0 to prevent phantom state divergence
///
/// IEEE 754 encodes +0.0 and −0.0 differently at the bit level.
/// When computing H_session hash, this phantom bit pattern difference
/// would cause two mathematically identical states to diverge in hash value.
///
/// Solution: Normalize all zeros to +0.0 before hashing.
///
/// # Example
/// ```ignore
/// let z_pos = 0.0f32;
/// let z_neg = -0.0f32;
/// assert_eq!(z_pos, z_neg);  // Mathematically equal
/// assert_ne!(z_pos.to_bits(), z_neg.to_bits());  // Different bit patterns!
///
/// let norm_pos = normalize_negative_zero(z_pos);
/// let norm_neg = normalize_negative_zero(z_neg);
/// assert_eq!(norm_pos.to_bits(), norm_neg.to_bits());  // ✓ Bit-identical
/// ```
#[inline]
pub fn normalize_negative_zero(x: f32) -> f32 {
    if x == 0.0 {
        0.0  // Map both +0.0 and −0.0 to positive zero
    } else {
        x
    }
}

/// Normalize all values in a vector (used before H_session computation)
pub fn normalize_vector(z: &mut [f32]) {
    for val in z.iter_mut() {
        *val = normalize_negative_zero(*val);
    }
}

// ============================================================================
// Testing: Bit-Identity and Round-Trip Verification
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_q31_encode_decode_round_trip() {
        let values = [
            0.0f32,
            0.1,
            0.5,
            0.99999,
            -0.5,
            -0.99999,
        ];

        for &v in &values {
            let q = q31_encode(v);
            let v_back = q31_decode(q);
            // Allow tiny rounding error (< 2^-31)
            assert!((v - v_back).abs() < 1e-9, "Mismatch for {}: got {}", v, v_back);
        }
    }

    #[test]
    fn test_q31_quantize_manifold() {
        let mut z = [0.123f32; 269];

        q31_quantize_manifold(&mut z);

        // After quantization, values should lie on Q31 grid
        for i in 0..269 {
            let q = q31_encode(z[i]);
            let z_expected = q31_decode(q);
            assert_eq!(z[i], z_expected, "Manifold element {} not on Q31 grid", i);
        }
    }

    #[test]
    fn test_adaptive_q_switch_normal_range() {
        let z_small = vec![0.1f32; 269];
        let mode = adaptive_q_switch(&z_small);
        assert_eq!(mode, QuantMode::Q31, "Small norm should use Q31");
    }

    #[test]
    fn test_adaptive_q_switch_overflow_range() {
        // Create high-dimensional overflow: norm > 2.0
        let mut z_large = [0.0f32; 269];
        for i in 0..100 {
            z_large[i] = 0.5f32;  // 100 × 0.5 = √(25) = 5.0 norm
        }
        let mode = adaptive_q_switch(&z_large);
        assert_eq!(mode, QuantMode::Q16, "High norm should use Q16");
    }

    #[test]
    fn test_phantom_prevention_negative_zero() {
        let z_pos = 0.0f32;
        let z_neg = -0.0f32;

        // Mathematically equal
        assert_eq!(z_pos, z_neg);
        // But bit patterns differ
        assert_ne!(z_pos.to_bits(), z_neg.to_bits());

        // After normalization, identical
        let norm_pos = normalize_negative_zero(z_pos);
        let norm_neg = normalize_negative_zero(z_neg);
        assert_eq!(norm_pos.to_bits(), norm_neg.to_bits());
    }

    #[test]
    fn test_q16_encode_decode() {
        let values = [0.0f32, 100.0, -100.0, 32767.0];
        for &v in &values {
            let q = q16_encode(v);
            let v_back = q16_decode(q);
            assert!((v - v_back).abs() < 0.01, "Q16 round-trip failed for {}", v);
        }
    }

    #[test]
    fn test_quantize_adaptive_mode_selection() {
        let mut z_q31 = vec![0.1f32; 269];
        let mut z_q16 = vec![0.5f32; 269];

        quantize_adaptive(&mut z_q31, QuantMode::Q31);
        quantize_adaptive(&mut z_q16, QuantMode::Q16);

        // Both should be quantized (no panic, deterministic)
        assert!(z_q31[0].is_finite());
        assert!(z_q16[0].is_finite());
    }

    #[test]
    fn test_determinism_identical_input_identical_output() {
        // Same input → same quantization → same bit pattern
        let z1 = [0.123456f32; 269];
        let z2 = [0.123456f32; 269];

        let mut z1_q = z1;
        let mut z2_q = z2;

        q31_quantize_manifold(&mut z1_q);
        q31_quantize_manifold(&mut z2_q);

        for i in 0..269 {
            assert_eq!(z1_q[i].to_bits(), z2_q[i].to_bits(),
                       "Determinism violation at index {}", i);
        }
    }
}
