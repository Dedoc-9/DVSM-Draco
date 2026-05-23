//! Physics Engine Layer (DVSM v3.3 Integration)
//!
//! **Phase**: Phase II — DVSM Kernel Port from BM Folder
//! **Status**: Foundation phase (fixed-point arithmetic locked)
//!
//! # Module Structure
//!
//! | Module | Purpose | Status |
//! |--------|---------|--------|
//! | `fixed_point` | Q31/Q16/Q64.64 codecs (bit-identical quantization) | ✅ |
//! | `dvsm_state` | State types (Z, S, G, W, H) | ⏳ Task #39 |
//! | `config` | SessionConfig + protocol locking | ⏳ Task #39 |
//! | `evolution` | 7-layer operators (L1-L7) | ⏳ Task #40 |
//! | `validator` | 3-invariant supervisor | ⏳ Task #41 |
//! | `regime_machine` | 5-state FSM (occupancy-driven) | ⏳ Task #43 |
//! | `rollback` | Forensic stack + replay support | ⏳ Task #41 |

pub mod fixed_point;
pub mod dvsm_state;
pub mod evolution;
pub mod validator;
pub mod bitfield_parser;
pub mod regime_fsm;

// Planned exports (will uncomment as modules are added)
// pub mod config;
// pub mod regime_machine;
// pub mod rollback;

// Re-export key types for library users
pub use fixed_point::{
    QuantMode,
    q31_encode,
    q31_decode,
    q31_quantize_vector,
    q31_quantize_manifold,
    q16_encode,
    q16_decode,
    q16_quantize_vector,
    q64_64_encode,
    q64_64_decode,
    adaptive_q_switch,
    quantize_adaptive,
    normalize_negative_zero,
    normalize_vector,
    // Constants
    Q31_SCALE,
    Q31_SCALE_INV,
    Q31_MIN,
    Q31_MAX,
    Q16_SCALE,
    Q16_SCALE_INV,
    Q64_64_SCALE,
    Q64_64_SCALE_INV,
};

pub use dvsm_state::{
    DvsmState,
    ProjectionBasis,
    MANIFOLD_DIM_PADDED,
    Z_NORM_MAX,
    S_NORM_MAX,
    NORM_WARNING_THRESHOLD,
};

pub use evolution::{
    SessionConfig,
    evolve_frame,
    compute_h_session,
};

pub use validator::{
    Supervisor,
    ValidationResult,
    StatusLevel,
};

pub use bitfield_parser::{
    BitfieldParser,
    TorsionArray,
    DestructionI24,
    DestructionRingBuffer,
    ParsingError,
};

pub use regime_fsm::{
    Regime,
    RegimeFsm,
};
