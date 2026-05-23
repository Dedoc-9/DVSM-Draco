/// src/interop/mod.rs
///
/// PHASE I.4a: DX12 Interop Module
/// Safe-Path bridge for non-invasive VRAM access
///
/// Pattern: IDXGIKeyedMutex + D3D12 Readback Heap (EAAC whitelist-compliant)
/// Result: Read-only destruction state snapshot without code injection

pub mod dx12_shared_handle;

pub use dx12_shared_handle::{
    SharedHandleReader,
    DestructionBitfield,
    TorsionSnapshot,
    I24,
};
