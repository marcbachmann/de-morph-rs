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
//!   [shape table                 : SHAPE_ENTRY_SIZE * num_shapes bytes]
//!     Interned analysis "shapes" — the (pos, source, aux,
//!     packed_features) tuple, i.e. everything an analysis carries
//!     EXCEPT the lemma. Only a few hundred distinct shapes exist across
//!     the whole corpus, so analyses reference one by a small id instead
//!     of repeating ~40 bits per record. See `Shape` below.
//!
//!   [analyses section            : 4 * num_analyses bytes]
//!     Repeated 4-byte u32 records: `lemma_id (20 bits) | shape_id
//!     (12 bits)`. See `AnalysisRecord` below. v1 used 12-byte records,
//!     v2-v4 packed to 8 bytes (a self-contained u64); v5 factors out
//!     the shape table so each record is just two indices = 4 bytes.
//! ```
//!
//! # v5 analysis record (4 bytes) and shape entry (6 bytes)
//!
//! Each analysis record is a little-endian u32:
//!
//! ```text
//!   bits 0-19    lemma_id   (20 bits, up to 1,048,576 lemmas)
//!   bits 20-31   shape_id   (12 bits, up to 4096 distinct shapes)
//! ```
//!
//! Each shape-table entry is 6 little-endian bytes:
//!
//! ```text
//!   bytes 0-3    packed_features   (full 32-bit PackedFeatures word)
//!   byte  4      pos               (UPOS enum, 5 bits used)
//!   byte  5      source (bits 0-1) | aux (bits 2-3)
//! ```
//!
//! v5 introduces the shape table: with only ~400 distinct
//! (pos, source, aux, packed_features) tuples across millions of
//! records, interning them halves the analyses section (8→4 bytes per
//! record). v4 stored the full analysis in each 8-byte record; v3
//! widened POS from 4→5 bits for the 17th UPOS tag (`Propn`).

/// File magic, including a trailing null byte so the length is 12.
pub const MAGIC: [u8; 12] = *b"DE-MORPH-RS\0";

/// Fixed header size.
pub const HEADER_SIZE: usize = 64;

/// Current format version. v3 widens the POS field from 4 → 5 bits to
/// admit the 17th UPOS tag (`Propn`). v2 packed the analysis record
/// from 12 bytes (v1) into 8 bytes; v3 keeps the 8-byte size.
pub const VERSION_MAJOR: u16 = 5;
pub const VERSION_MINOR: u16 = 0;

/// Size of one analysis record on disk (bytes): `lemma_id | shape_id`.
pub const ANALYSIS_RECORD_SIZE: usize = 4;

/// Size of one shape-table entry on disk (bytes).
pub const SHAPE_ENTRY_SIZE: usize = 6;

/// Maximum number of lemmas this format can address (20-bit `lemma_id`).
pub const MAX_LEMMA_ID: u32 = (1 << 20) - 1;

/// Maximum number of distinct shapes (12-bit `shape_id`).
pub const MAX_SHAPE_ID: u32 = (1 << 12) - 1;

/// One analysis record: a (lemma, shape) index pair. The wire form is a
/// little-endian u32 with `lemma_id` in the low 20 bits and `shape_id`
/// in the high 12 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisRecord {
    pub lemma_id: u32,
    pub shape_id: u16,
}

impl AnalysisRecord {
    pub const SIZE: usize = ANALYSIS_RECORD_SIZE;

    /// Pack into the on-disk u32 representation.
    pub fn to_u32(self) -> u32 {
        debug_assert!(self.lemma_id <= MAX_LEMMA_ID, "lemma_id overflow");
        debug_assert!((self.shape_id as u32) <= MAX_SHAPE_ID, "shape_id overflow");
        (self.lemma_id & 0xF_FFFF) | (((self.shape_id as u32) & 0xFFF) << 20)
    }

    /// Unpack from the on-disk u32 representation.
    #[inline]
    pub fn from_u32(bits: u32) -> Self {
        Self {
            lemma_id: bits & 0xF_FFFF,
            shape_id: ((bits >> 20) & 0xFFF) as u16,
        }
    }

    /// Serialise to 4 bytes (little-endian).
    pub fn to_bytes(self) -> [u8; ANALYSIS_RECORD_SIZE] {
        self.to_u32().to_le_bytes()
    }

    /// Deserialise from 4 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        debug_assert!(bytes.len() >= ANALYSIS_RECORD_SIZE);
        Self::from_u32(u32::from_le_bytes(bytes[0..4].try_into().unwrap()))
    }
}

/// An interned analysis "shape": everything an analysis carries except
/// the lemma. Stored once in the shape table and referenced by id from
/// each [`AnalysisRecord`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shape {
    pub packed_features: u32,
    pub pos: u8,
    /// Source code (2 bits) and aux code (2 bits) share one byte.
    pub source: u8,
    pub aux: u8,
}

impl Shape {
    /// Serialise to 6 bytes (little-endian).
    pub fn to_bytes(self) -> [u8; SHAPE_ENTRY_SIZE] {
        debug_assert!(self.pos <= 0x1F && self.source <= 0x3 && self.aux <= 0x3);
        let mut out = [0u8; SHAPE_ENTRY_SIZE];
        out[0..4].copy_from_slice(&self.packed_features.to_le_bytes());
        out[4] = self.pos;
        out[5] = (self.source & 0x3) | ((self.aux & 0x3) << 2);
        out
    }

    /// Deserialise from 6 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        debug_assert!(bytes.len() >= SHAPE_ENTRY_SIZE);
        let packed_features = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let pos = bytes[4];
        let b5 = bytes[5];
        Self {
            packed_features,
            pos,
            source: b5 & 0x3,
            aux: (b5 >> 2) & 0x3,
        }
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
    fn record_size_is_4_bytes_on_disk() {
        assert_eq!(ANALYSIS_RECORD_SIZE, 4);
        assert_eq!(SHAPE_ENTRY_SIZE, 6);
    }

    #[test]
    fn record_roundtrip() {
        let r = AnalysisRecord {
            lemma_id: 0x0_ABCD_E, // 20-bit value
            shape_id: 0x0AB,      // 12-bit value
        };
        let bytes = r.to_bytes();
        assert_eq!(bytes.len(), 4);
        assert_eq!(AnalysisRecord::from_bytes(&bytes), r);
    }

    #[test]
    fn record_packs_to_known_layout() {
        let r = AnalysisRecord {
            lemma_id: 0xF_FFFF, // max 20-bit value
            shape_id: 0xFFF,    // max 12-bit value
        };
        // lemma_id in bits 0-19, shape_id in bits 20-31.
        assert_eq!(r.to_u32(), 0xFFFF_FFFF);
        assert_eq!(AnalysisRecord::from_u32(0xFFFF_FFFF), r);
        // lemma_id alone occupies only the low 20 bits.
        assert_eq!(
            AnalysisRecord {
                lemma_id: 0xF_FFFF,
                shape_id: 0
            }
            .to_u32(),
            0x000F_FFFF
        );
    }

    #[test]
    fn shape_roundtrips_all_codes() {
        for source in 0u8..=3 {
            for aux in 0u8..=3 {
                let s = Shape {
                    packed_features: 0xDEAD_BEEF,
                    pos: 0x1F,
                    source,
                    aux,
                };
                assert_eq!(Shape::from_bytes(&s.to_bytes()), s);
            }
        }
    }

    #[test]
    fn pack_unpack_roundtrip() {
        let value = pack_fst_value(42, 1024);
        assert_eq!(unpack_fst_value(value), (42, 1024));

        let value = pack_fst_value(u32::MAX, u32::MAX);
        assert_eq!(unpack_fst_value(value), (u32::MAX, u32::MAX));
    }
}
