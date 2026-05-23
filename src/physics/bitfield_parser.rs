//! Bitfield Parser: Destruction Events → Torsion Array
//!
//! **Module**: Event Input Layer (Observer → Simulator Bridge)
//! **Reference**: BM Folder BITFIELD_SPEC.md (destruction event encoding)
//! **Purpose**: Parse destruction bitfield (I24 events + CRC-32) into torsion array for L1 injection
//!
//! # Data Flow
//!
//! ```ignore
//! DX12 VRAM Reader (destruction events)
//!   ↓
//!   [Bitfield: packed I24 samples + CRC-32 checksum]
//!   ↓
//! BitfieldParser::parse_destruction()
//!   ├─ Unpack I24 → i32 (sign-extend)
//!   ├─ Validate CRC-32
//!   ├─ Extract occupancy (# events)
//!   └─ Compute torsion array (269-element impulse vector)
//!   ↓
//!   [TorsionArray: {occupancy, impulses[0..268]}]
//!   ↓
//! L1 (Load Operator): apply_load(z, occupancy, torsion)
//! ```
//!
//! # Critical Design Notes
//!
//! **I24 Sign-Extension**: 3-byte signed integer → 32-bit signed
//! - Bitfield packs destruction magnitudes as I24 (-8M to +8M range)
//! - Must sign-extend: if bit 23 set, all upper bits become 1
//! - Example: 0xFF_FFFF (I24) → 0xFFFFFFFF (i32, -1)
//!
//! **CRC-32 Validation**: Protects against transmission errors
//! - Checksum computed over all I24 samples + occupancy count
//! - If CRC fails, return Err(ParsingError::CrcMismatch)
//! - Never partially process corrupted bitfield
//!
//! **Ring Buffer Semantics**: Occupancy count determines valid events
//! - Ring buffer can hold 0-128 destruction events per frame
//! - Occupancy = actual event count (may be < 128)
//! - Pad with zeros if occupancy < 128 (ensures consistent array size)
//!
//! **Torsion Array**: 269-element impulse vector
//! - Each element = destruction impulse on that manifold dimension
//! - Computed via projection of I24 samples onto manifold basis
//! - Magnitude scales with occupancy (more events → stronger impulses)

use std::collections::VecDeque;

/// Destruction event in I24 format (3 bytes, packed)
#[derive(Debug, Clone, Copy)]
pub struct DestructionI24([u8; 3]);

impl DestructionI24 {
    /// Unpack I24 to signed i32 with sign-extension
    pub fn to_i32(&self) -> i32 {
        let [b0, b1, b2] = self.0;
        let val = ((b2 as i32) << 16) | ((b1 as i32) << 8) | (b0 as i32);
        // Sign-extend from bit 23
        if val & 0x80_0000 != 0 {
            val | (0xFF00_0000u32 as i32)  // Set upper 8 bits to 1 (negative)
        } else {
            val & 0x00FF_FFFF  // Keep upper 8 bits as 0 (positive)
        }
    }

    /// Create from i32 (clamped to I24 range)
    pub fn from_i32(val: i32) -> Self {
        let clamped = val.clamp(-0x80_0000, 0x7F_FFFF);
        let b0 = (clamped & 0xFF) as u8;
        let b1 = ((clamped >> 8) & 0xFF) as u8;
        let b2 = ((clamped >> 16) & 0xFF) as u8;
        DestructionI24([b0, b1, b2])
    }
}

/// Parsed destruction torsion array (269-element impulse vector)
#[derive(Debug, Clone, PartialEq)]
pub struct TorsionArray {
    /// Number of actual destruction events (0-128)
    pub occupancy: u32,
    /// Impulse vector: one per manifold dimension
    pub impulses: [f32; 269],
}

impl TorsionArray {
    /// Create zero-impulse array (no destruction)
    pub fn zero() -> Self {
        TorsionArray {
            occupancy: 0,
            impulses: [0.0; 269],
        }
    }

    /// Norm of impulse vector
    pub fn norm(&self) -> f32 {
        let norm_sq: f32 = self.impulses.iter().map(|&x| x * x).sum();
        norm_sq.sqrt()
    }
}

/// Parsing errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParsingError {
    /// CRC-32 checksum mismatch
    CrcMismatch,
    /// Occupancy count exceeds 128
    OccupancyOverflow,
    /// Bitfield truncated or incomplete
    Truncated,
    /// Invalid bitfield structure
    InvalidFormat,
}

/// Ring buffer for destruction events (circular queue)
pub struct DestructionRingBuffer {
    /// Circular queue of I24 events
    events: VecDeque<DestructionI24>,
    /// Maximum capacity (typically 128)
    capacity: usize,
}

impl DestructionRingBuffer {
    /// Create new ring buffer with given capacity
    pub fn new(capacity: usize) -> Self {
        DestructionRingBuffer {
            events: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Push destruction event to ring buffer
    pub fn push(&mut self, event: DestructionI24) -> Result<(), ParsingError> {
        if self.events.len() >= self.capacity {
            return Err(ParsingError::OccupancyOverflow);
        }
        self.events.push_back(event);
        Ok(())
    }

    /// Get current occupancy (number of buffered events)
    pub fn occupancy(&self) -> u32 {
        self.events.len() as u32
    }

    /// Clear all events
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Get snapshot of current events (for frame processing)
    pub fn snapshot(&self) -> Vec<DestructionI24> {
        self.events.iter().copied().collect()
    }
}

/// Bitfield parser and validator
pub struct BitfieldParser {
    /// CRC-32 polynomial (standard IEEE 802.3)
    crc_poly: u32,
}

impl BitfieldParser {
    /// Create new parser
    pub fn new() -> Self {
        BitfieldParser {
            crc_poly: 0x04C1_1DB7,  // IEEE 802.3 CRC-32
        }
    }

    /// Parse bitfield: I24 events + CRC-32 checksum
    ///
    /// **Input**:
    /// - bitfield: Byte array containing packed I24 events + CRC-32 tail
    /// - Format: [I24_0 (3B), I24_1 (3B), ..., I24_127 (3B), occupancy (4B), CRC-32 (4B)]
    /// - Total: 128×3 + 4 + 4 = 400 bytes
    ///
    /// **Output**:
    /// - TorsionArray with parsed occupancy and computed impulses
    ///
    /// **Errors**:
    /// - CrcMismatch: Checksum validation failed
    /// - OccupancyOverflow: Occupancy > 128
    /// - Truncated: Bitfield too short
    pub fn parse_destruction(&self, bitfield: &[u8]) -> Result<TorsionArray, ParsingError> {
        // Minimum size: 128×3 (I24 events) + 4 (occupancy) + 4 (CRC-32)
        const BITFIELD_SIZE: usize = 128 * 3 + 4 + 4;
        if bitfield.len() < BITFIELD_SIZE {
            return Err(ParsingError::Truncated);
        }

        // Extract occupancy (4 bytes before CRC-32)
        let occupancy_offset = BITFIELD_SIZE - 8;
        let occupancy_bytes = [
            bitfield[occupancy_offset],
            bitfield[occupancy_offset + 1],
            bitfield[occupancy_offset + 2],
            bitfield[occupancy_offset + 3],
        ];
        let occupancy = u32::from_le_bytes(occupancy_bytes);

        // Validate occupancy
        if occupancy > 128 {
            return Err(ParsingError::OccupancyOverflow);
        }

        // Extract CRC-32 checksum (last 4 bytes)
        let crc_offset = BITFIELD_SIZE - 4;
        let crc_bytes = [
            bitfield[crc_offset],
            bitfield[crc_offset + 1],
            bitfield[crc_offset + 2],
            bitfield[crc_offset + 3],
        ];
        let stored_crc = u32::from_le_bytes(crc_bytes);

        // Compute CRC-32 over data (I24 events + occupancy)
        let computed_crc = self.crc32(&bitfield[0..occupancy_offset + 4]);

        // Validate CRC
        if computed_crc != stored_crc {
            return Err(ParsingError::CrcMismatch);
        }

        // Parse I24 events
        let mut events = Vec::new();
        for i in 0..occupancy as usize {
            let offset = i * 3;
            if offset + 3 > bitfield.len() {
                return Err(ParsingError::Truncated);
            }
            let event = DestructionI24([bitfield[offset], bitfield[offset + 1], bitfield[offset + 2]]);
            events.push(event);
        }

        // Compute torsion array from events
        let torsion = self.compute_torsion_array(&events, occupancy);

        Ok(torsion)
    }

    /// Compute CRC-32 checksum over data
    fn crc32(&self, data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;

        for &byte in data {
            crc ^= byte as u32;
            for _ in 0..8 {
                if crc & 1 == 1 {
                    crc = (crc >> 1) ^ self.crc_poly;
                } else {
                    crc >>= 1;
                }
            }
        }

        crc ^ 0xFFFF_FFFF
    }

    /// Compute 269-element torsion array from destruction events
    ///
    /// **Algorithm**:
    /// 1. Unpack I24 events to i32 destruction magnitudes
    /// 2. Project onto manifold basis (269 dimensions)
    /// 3. Scale by occupancy (more events → stronger impulses)
    /// 4. Normalize to prevent overflow
    ///
    /// **Cost**: O(occupancy × 269) ≈ 35K operations for full buffer
    fn compute_torsion_array(&self, events: &[DestructionI24], occupancy: u32) -> TorsionArray {
        let mut impulses = [0.0f32; 269];

        if occupancy == 0 {
            return TorsionArray { occupancy, impulses };
        }

        // Project each event onto manifold dimensions
        for (idx, &event) in events.iter().enumerate() {
            let magnitude = event.to_i32() as f32;

            // Distribute event energy across manifold dimensions
            // Simple projection: cyclic distribution (event i → dimensions i % 269, (i+1) % 269, ...)
            for dim in 0..269 {
                let phase = ((idx * 7 + dim) % 269) as f32 / 269.0;  // Pseudo-random phase
                let contribution = magnitude * phase.sin();  // Sinusoidal distribution
                impulses[dim] += contribution;
            }
        }

        // Normalize by occupancy to keep magnitude bounded
        let occupancy_scale = 1.0 / ((occupancy as f32).sqrt());
        for impulse in impulses.iter_mut() {
            *impulse *= occupancy_scale;
        }

        TorsionArray { occupancy, impulses }
    }
}

// ============================================================================
// Testing: Parsing, CRC Validation, Ring Buffer
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i24_sign_extension_positive() {
        let event = DestructionI24([0x00, 0x00, 0x7F]);  // 0x7F_0000
        assert_eq!(event.to_i32(), 0x7F_0000);
    }

    #[test]
    fn test_i24_sign_extension_negative() {
        let event = DestructionI24([0xFF, 0xFF, 0xFF]);  // 0xFF_FFFF (all bits set)
        assert_eq!(event.to_i32(), -1i32);
    }

    #[test]
    fn test_i24_sign_extension_boundary() {
        let event = DestructionI24([0x00, 0x00, 0x80]);  // 0x80_0000 (sign bit)
        assert_eq!(event.to_i32(), -0x80_0000);
    }

    #[test]
    fn test_i24_from_i32() {
        let val = 0x12_3456i32;
        let event = DestructionI24::from_i32(val);
        assert_eq!(event.to_i32(), val);
    }

    #[test]
    fn test_i24_clamp_to_range() {
        let event_overflow = DestructionI24::from_i32(0x1000_0000);  // Beyond I24 range
        assert!(event_overflow.to_i32().abs() <= 0x80_0000);
    }

    #[test]
    fn test_ring_buffer_push() {
        let mut rb = DestructionRingBuffer::new(4);
        let event = DestructionI24([0x00, 0x00, 0x7F]);

        assert!(rb.push(event).is_ok());
        assert_eq!(rb.occupancy(), 1);
    }

    #[test]
    fn test_ring_buffer_overflow() {
        let mut rb = DestructionRingBuffer::new(2);
        let event = DestructionI24([0x00, 0x00, 0x7F]);

        rb.push(event).unwrap();
        rb.push(event).unwrap();

        // Third push should fail (capacity exceeded)
        assert_eq!(rb.push(event), Err(ParsingError::OccupancyOverflow));
    }

    #[test]
    fn test_ring_buffer_clear() {
        let mut rb = DestructionRingBuffer::new(4);
        let event = DestructionI24([0x00, 0x00, 0x7F]);

        rb.push(event).unwrap();
        assert_eq!(rb.occupancy(), 1);

        rb.clear();
        assert_eq!(rb.occupancy(), 0);
    }

    #[test]
    fn test_torsion_array_zero() {
        let ta = TorsionArray::zero();
        assert_eq!(ta.occupancy, 0);
        assert_eq!(ta.norm(), 0.0);
    }

    #[test]
    fn test_torsion_array_norm() {
        let mut ta = TorsionArray::zero();
        ta.impulses[0] = 3.0;
        ta.impulses[1] = 4.0;  // 3-4-5 triangle
        assert!((ta.norm() - 5.0).abs() < 1e-5);
    }

    #[test]
    fn test_bitfield_parser_empty() {
        let parser = BitfieldParser::new();
        let mut bitfield = vec![0u8; 128 * 3 + 4 + 4];

        // Set occupancy to 0
        bitfield[128 * 3] = 0;
        bitfield[128 * 3 + 1] = 0;
        bitfield[128 * 3 + 2] = 0;
        bitfield[128 * 3 + 3] = 0;

        // Compute CRC for zero occupancy
        let crc = parser.crc32(&bitfield[0..128 * 3 + 4]);
        bitfield[128 * 3 + 4] = (crc & 0xFF) as u8;
        bitfield[128 * 3 + 5] = ((crc >> 8) & 0xFF) as u8;
        bitfield[128 * 3 + 6] = ((crc >> 16) & 0xFF) as u8;
        bitfield[128 * 3 + 7] = ((crc >> 24) & 0xFF) as u8;

        let result = parser.parse_destruction(&bitfield).unwrap();
        assert_eq!(result.occupancy, 0);
        assert_eq!(result.norm(), 0.0);
    }

    #[test]
    fn test_bitfield_parser_crc_mismatch() {
        let parser = BitfieldParser::new();
        let mut bitfield = vec![0u8; 128 * 3 + 4 + 4];

        // Set bad CRC
        bitfield[128 * 3 + 4] = 0xFF;
        bitfield[128 * 3 + 5] = 0xFF;
        bitfield[128 * 3 + 6] = 0xFF;
        bitfield[128 * 3 + 7] = 0xFF;

        assert_eq!(parser.parse_destruction(&bitfield), Err(ParsingError::CrcMismatch));
    }

    #[test]
    fn test_bitfield_parser_occupancy_overflow() {
        let parser = BitfieldParser::new();
        let mut bitfield = vec![0u8; 128 * 3 + 4 + 4];

        // Set occupancy to 129 (overflow)
        let occupancy = 129u32;
        bitfield[128 * 3] = (occupancy & 0xFF) as u8;
        bitfield[128 * 3 + 1] = ((occupancy >> 8) & 0xFF) as u8;

        // CRC won't match but we fail on occupancy first
        assert_eq!(parser.parse_destruction(&bitfield), Err(ParsingError::OccupancyOverflow));
    }

    #[test]
    fn test_bitfield_parser_truncated() {
        let parser = BitfieldParser::new();
        let bitfield = vec![0u8; 100];  // Too short

        assert_eq!(parser.parse_destruction(&bitfield), Err(ParsingError::Truncated));
    }

    #[test]
    fn test_bitfield_parser_with_events() {
        let parser = BitfieldParser::new();
        let mut bitfield = vec![0u8; 128 * 3 + 4 + 4];

        // Add 3 test events
        let events = [
            DestructionI24([0x00, 0x00, 0x7F]),
            DestructionI24([0xFF, 0xFF, 0xFF]),
            DestructionI24([0x00, 0x80, 0x00]),
        ];

        for (i, &event) in events.iter().enumerate() {
            bitfield[i * 3] = event.0[0];
            bitfield[i * 3 + 1] = event.0[1];
            bitfield[i * 3 + 2] = event.0[2];
        }

        // Set occupancy to 3
        let occupancy = 3u32;
        bitfield[128 * 3] = (occupancy & 0xFF) as u8;
        bitfield[128 * 3 + 1] = ((occupancy >> 8) & 0xFF) as u8;
        bitfield[128 * 3 + 2] = 0;
        bitfield[128 * 3 + 3] = 0;

        // Compute CRC
        let crc = parser.crc32(&bitfield[0..128 * 3 + 4]);
        bitfield[128 * 3 + 4] = (crc & 0xFF) as u8;
        bitfield[128 * 3 + 5] = ((crc >> 8) & 0xFF) as u8;
        bitfield[128 * 3 + 6] = ((crc >> 16) & 0xFF) as u8;
        bitfield[128 * 3 + 7] = ((crc >> 24) & 0xFF) as u8;

        let result = parser.parse_destruction(&bitfield).unwrap();
        assert_eq!(result.occupancy, 3);
        assert!(result.norm() > 0.0);
    }
}
