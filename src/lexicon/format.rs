//! On-disk format constants and record layout.
//!
//! All multi-byte integers are little-endian. The format is designed
//! to be readable as a flat byte slice (memory-mapped or owned) with
//! no per-field parsing: lemma offsets and analysis records are fixed
//! size and accessed by index.
//!
//! # Layout
//!
//! ```text
//!   [header                                              : 64 bytes]
//!     magic                       : [u8; 12]   "DE-MORPH-RS\0"
//!     version_major               : u16        = 2
//!     version_minor               : u16        = 0
//!     flags                       : u32        reserved, must be 0
//!     num_lemmas                  : u32
//!     num_analyses                : u32
//!     lemma_offsets_offset        : u32        offset of lemma_offsets section
//!     lemma_bytes_offset          : u32        offset of lemma_bytes section
//!     analyses_offset             : u32        offset of analyses section
//!     analyses_end                : u32        end byte (file length)
//!     _reserved                   : [u8; 12]   zeroed
//!
//!   [lemma_offsets section                : 4 * (num_lemmas + 1) bytes]
//!     u32 offset for lemma 0, 1, ..., num_lemmas
//!     The (num_lemmas)-th entry is a sentinel: it equals the length
//!     of `lemma_bytes`. Each offset is relative to `lemma_bytes_offset`.
//!
//!   [lemma_bytes section                  : variable]
//!     UTF-8 bytes of all lemmas concatenated. Length of lemma N is
//!     `lemma_offsets[N+1] - lemma_offsets[N]`.
//!
//!   [analyses section            : 8 * num_analyses bytes]
//!     Repeated 8-byte u64 records. See `AnalysisRecord` bit layout
//!     below. Version 1 of the format used 12-byte records with an
//!     explicit `#[repr(C)]` struct; v2 packs the same information
//!     into a single u64 for a 33% size reduction on this section.
//! ```
//!
//! # Bit layout of the 8-byte analysis record (v4)
//!
//! Each record is a single little-endian u64:
//!
//! ```text
//!   bits 0-31    packed_features   (32 bits, full PackedFeatures word)
//!   bits 32-51   lemma_id          (20 bits, supports up to 1,048,576 lemmas)
//!   bits 52-56   pos               (5 bits, 32 POS values — UPOS enum)
//!   bits 57-58   source            (2 bits, 4 Source values)
//!   bits 59-60   aux               (2 bits, 0=unset/1=Haben/2=Sein/3=Both)
//!   bits 61-63   reserved          (3 bits, must be 0)
//! ```
//!
//! v4 claims two of v3's reserved bits for the verb's perfect-tense
//! auxiliary (`Hilfsverb`): the `PackedFeatures` word is full, so this
//! lexical property rides in the record instead. The encoding is
//! bit-compatible with v3 (a v3 record's zero reserved bits decode as
//! `aux` unset), but the version is bumped so the meaning is explicit.
//! v3 widened POS from 4→5 bits for the 17th UPOS tag (`Propn`).

/// File magic, including a trailing null byte so the length is 12.
pub const MAGIC: [u8; 12] = *b"DE-MORPH-RS\0";

/// Fixed header size.
pub const HEADER_SIZE: usize = 64;

/// Current format version. v3 widens the POS field from 4 → 5 bits to
/// admit the 17th UPOS tag (`Propn`). v2 packed the analysis record
/// from 12 bytes (v1) into 8 bytes; v3 keeps the 8-byte size.
pub const VERSION_MAJOR: u16 = 4;
pub const VERSION_MINOR: u16 = 0;

/// Size of one analysis record on disk (bytes).
pub const ANALYSIS_RECORD_SIZE: usize = 8;

/// Maximum number of lemmas this format can address. Determined by
/// the 20-bit `lemma_id` field in the record.
pub const MAX_LEMMA_ID: u32 = (1 << 20) - 1;

/// Packed 8-byte record for one analysis. The wire form is a single
/// little-endian u64; `lemma_id`, `pos`, `source`, and the full 32-bit
/// `packed_features` are extracted with shifts and masks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisRecord {
    pub lemma_id: u32,
    pub pos: u8,
    pub source: u8,
    pub packed_features: u32,
    /// Perfect-tense auxiliary code: 0=unset, 1=Haben, 2=Sein, 3=Both.
    /// Stored in reserved bits 59-60 (the PackedFeatures word is full).
    pub aux: u8,
}

impl AnalysisRecord {
    pub const SIZE: usize = ANALYSIS_RECORD_SIZE;

    /// Pack the record into the on-disk u64 representation.
    pub fn to_u64(self) -> u64 {
        debug_assert!(self.lemma_id <= MAX_LEMMA_ID, "lemma_id overflow");
        debug_assert!(self.pos <= 0x1F, "pos overflow (max 5 bits = 31)");
        debug_assert!(self.source <= 0x3, "source overflow");
        debug_assert!(self.aux <= 0x3, "aux overflow (max 2 bits = 3)");
        let mut bits: u64 = 0;
        bits |= self.packed_features as u64;
        bits |= ((self.lemma_id as u64) & 0xF_FFFF) << 32;
        bits |= ((self.pos as u64) & 0x1F) << 52;
        bits |= ((self.source as u64) & 0x3) << 57;
        bits |= ((self.aux as u64) & 0x3) << 59;
        bits
    }

    /// Unpack the record from the on-disk u64 representation.
    #[inline]
    pub fn from_u64(bits: u64) -> Self {
        Self {
            packed_features: (bits & 0xFFFF_FFFF) as u32,
            lemma_id: ((bits >> 32) & 0xF_FFFF) as u32,
            pos: ((bits >> 52) & 0x1F) as u8,
            source: ((bits >> 57) & 0x3) as u8,
            aux: ((bits >> 59) & 0x3) as u8,
        }
    }

    /// Serialise to 8 bytes (little-endian).
    pub fn to_bytes(self) -> [u8; ANALYSIS_RECORD_SIZE] {
        self.to_u64().to_le_bytes()
    }

    /// Deserialise from 8 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        debug_assert!(bytes.len() >= ANALYSIS_RECORD_SIZE);
        Self::from_u64(u64::from_le_bytes(bytes[0..8].try_into().unwrap()))
    }
}

/// Pack `(count, offset)` into the u64 value stored in the FST.
#[inline]
pub fn pack_fst_value(count: u32, offset: u32) -> u64 {
    ((count as u64) << 32) | (offset as u64)
}

/// Unpack the u64 value into `(count, offset)`.
#[inline]
pub fn unpack_fst_value(value: u64) -> (u32, u32) {
    let count = (value >> 32) as u32;
    let offset = (value & 0xFFFF_FFFF) as u32;
    (count, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_size_is_8_bytes_on_disk() {
        assert_eq!(ANALYSIS_RECORD_SIZE, 8);
    }

    #[test]
    fn record_roundtrip() {
        // Use values that fit the v3 bit budget: lemma_id ≤ 20 bits,
        // pos ≤ 5 bits, source ≤ 2 bits, packed_features ≤ 32 bits.
        let r = AnalysisRecord {
            lemma_id: 0x0_ABCD_E,           // 20-bit value
            pos: 0x10,                       // 5-bit value (16 = Propn)
            source: 0x2,                     // 2-bit value
            packed_features: 0xCAFE_BABE,    // any u32
            aux: 0x2,                        // 2-bit value (Sein)
        };
        let bytes = r.to_bytes();
        assert_eq!(bytes.len(), 8);
        assert_eq!(AnalysisRecord::from_bytes(&bytes), r);
    }

    #[test]
    fn record_aux_roundtrips_all_codes() {
        for aux in 0u8..=3 {
            let r = AnalysisRecord {
                lemma_id: 7,
                pos: 1,
                source: 0,
                packed_features: 0x1234_5678,
                aux,
            };
            assert_eq!(AnalysisRecord::from_bytes(&r.to_bytes()).aux, aux);
        }
    }

    #[test]
    fn record_packs_to_known_layout() {
        // Verify the bit positions documented in the module header (v4).
        let r = AnalysisRecord {
            lemma_id: 0xF_FFFF,              // max 20-bit value
            pos: 0x1F,                       // max 5-bit value
            source: 0x3,                     // max 2-bit value
            packed_features: 0xFFFF_FFFF,    // max u32
            aux: 0x3,                        // max 2-bit value
        };
        let u = r.to_u64();
        // Bits 0-31:  packed_features = 0xFFFFFFFF
        // Bits 32-51: lemma_id        = 0xFFFFF (20 bits)
        // Bits 52-56: pos             = 0x1F (5 bits)
        // Bits 57-58: source          = 0x3
        // Bits 59-60: aux             = 0x3
        // Bits 61-63: reserved        = 0
        // Total mask: 0x1FFF_FFFF_FFFF_FFFF
        assert_eq!(u, 0x1FFF_FFFF_FFFF_FFFF);
    }

    #[test]
    fn record_size_is_8_bytes() {
        assert_eq!(ANALYSIS_RECORD_SIZE, 8);
    }

    #[test]
    fn pack_unpack_roundtrip() {
        let value = pack_fst_value(42, 1024);
        assert_eq!(unpack_fst_value(value), (42, 1024));

        let value = pack_fst_value(u32::MAX, u32::MAX);
        assert_eq!(unpack_fst_value(value), (u32::MAX, u32::MAX));
    }
}
