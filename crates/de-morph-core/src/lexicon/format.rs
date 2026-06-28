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
//!     version_major               : u16        = 7        (offset 12)
//!     version_minor               : u16        = 0        (offset 14)
//!     flags                       : u32        reserved   (offset 16)
//!     num_lemmas                  : u32                   (offset 20)
//!     num_analyses                : u32        total readings (offset 24)
//!     lemma_offsets_offset        : u32                   (offset 28)
//!     lemma_bytes_offset          : u32                   (offset 32)
//!     analyses_offset             : u32        start of bit-packed groups (offset 36)
//!     analyses_end                : u32        end byte / file length (offset 40)
//!     num_shapes                  : u32                   (offset 44)
//!     shape_table_offset          : u32                   (offset 48)
//!     lemma_bits                  : u8         bit width of a lemma_id (offset 52)
//!     shape_bits                  : u8         bit width of a shape_id (offset 53)
//!     num_shape_sets              : u32        distinct shape-sets (offset 54)
//!     shape_set_dict_offset       : u32                   (offset 58)
//!     set_id_bits                 : u8         bit width of a shape_set_id (offset 62)
//!     offset_bits                 : u8         bit width of a lemma byte offset (offset 63)
//!
//!   [lemma_offsets section   : ceil((num_lemmas + 1) * offset_bits / 8) bytes]
//!     Bit-packed (LSB-first) array of (num_lemmas + 1) offsets, each
//!     `offset_bits` wide, relative to `lemma_bytes_offset`. The last is a
//!     sentinel equal to `len(lemma_bytes)`. Lemma N spans
//!     [offset[N], offset[N+1]).
//!
//!   [lemma_bytes section                  : variable]
//!     UTF-8 bytes of all lemmas concatenated, verbatim and contiguous so
//!     the loader can hand back borrowed &str (zero-copy lemmas).
//!
//!   [shape table                 : SHAPE_ENTRY_SIZE * num_shapes bytes]
//!     Interned analysis "shapes" — the (pos, source, aux,
//!     packed_features) tuple, everything an analysis carries EXCEPT the
//!     lemma. A few hundred distinct shapes; referenced by `shape_id`.
//!
//!   [shape-set dictionary]
//!     The distinct shape-SETS, deduplicated. A surface's analyses for one
//!     lemma form a set of shape_ids governed by the inflection pattern,
//!     not the lemma ("großen"/"schönen" share one set), so only ~1k
//!     distinct sets exist across the corpus. Layout:
//!       set_offsets : (num_shape_sets + 1) u16   indices into payload
//!       payload     : sum(|set|) u16             shape_ids; set `s` is
//!                                                 payload[off[s]..off[s+1]]
//!
//!   [analyses (groups) section   : variable, bit-packed]
//!     Per surface, a byte-aligned LSB-first run of its (lemma, shape-set)
//!     groups. The FST value `(group_count, byte_offset)` gives a
//!     surface's group count and the byte offset of its run. See "v7
//!     group encoding" below.
//! ```
//!
//! # v7 group encoding (lemma- and shape-set-factored)
//!
//! v6 factored the lemma out of each group but still stored that group's
//! explicit list of shape_ids. v7 also interns the shape-SET, so each
//! group is just two indices, written LSB-first then byte-aligned per
//! surface:
//!
//! ```text
//!   per group :  lemma_id      : lemma_bits
//!                shape_set_id  : set_id_bits
//! ```
//!
//! 99%+ of surfaces are a single group (single lemma). The decoder reads
//! `group_count` groups from the surface's byte-aligned run; for each it
//! expands `shape_set_id` via the dictionary into that group's shape_ids
//! and emits one analysis per (lemma_id, shape_id). Field widths are
//! data-fit and recorded in the header. Random access is preserved: the
//! FST offset seeks straight to the surface's run.
//!
//! Each shape-table entry is 6 little-endian bytes:
//!
//! ```text
//!   bytes 0-3    packed_features   (full 32-bit PackedFeatures word)
//!   byte  4      pos               (UPOS enum, 5 bits used)
//!   byte  5      source (bits 0-1) | aux (bits 2-3)
//! ```
//!
//! History: v6 lemma-factored each group + bit-packed (lemma_id once per
//! distinct lemma + a new_lemma flag). v5 introduced the shape table; v3
//! widened POS 4→5 bits for `Propn`. v7 adds shape-set interning and
//! bit-packs the lemma-offset table.

/// File magic, including a trailing null byte so the length is 12.
pub const MAGIC: [u8; 12] = *b"DE-MORPH-RS\0";

/// Fixed header size.
pub const HEADER_SIZE: usize = 64;

/// Current format version. v7 interns shape-SETS: each (surface, lemma)
/// group is a `(lemma_id, shape_set_id)` pair, with the distinct shape
/// lists deduplicated into a small dictionary; it also bit-packs the
/// lemma-offset table. v6 lemma-factored the analyses section; v5 interned
/// the (pos, source, aux, features) shape; v3 widened POS 4→5 bits for
/// `Propn`; v2 packed the v1 12-byte record to 8 bytes.
pub const VERSION_MAJOR: u16 = 7;
pub const VERSION_MINOR: u16 = 0;

/// Size of the logical analysis record (`lemma_id | shape_id`) as a
/// packed u32. Up to v5 this was also the on-disk record size; v6
/// bit-packs the analyses section, so this is now only the width of the
/// in-memory [`AnalysisRecord`] codec, not a per-record on-disk stride.
pub const ANALYSIS_RECORD_SIZE: usize = 4;

/// Size of one shape-table entry on disk (bytes).
pub const SHAPE_ENTRY_SIZE: usize = 6;

/// Maximum number of lemmas this format can address (20-bit `lemma_id`).
pub const MAX_LEMMA_ID: u32 = (1 << 20) - 1;

/// Maximum number of distinct shapes (12-bit `shape_id`).
pub const MAX_SHAPE_ID: u32 = (1 << 12) - 1;

/// One analysis record: a (lemma, shape) index pair. This is the
/// in-memory/logical unit; `to_u32`/`to_bytes` pack it as a u32
/// (`lemma_id` low 20 bits, `shape_id` high 12 bits), which was the v5
/// on-disk record. **In v6 the on-disk form is bit-packed** (see the
/// module-level "v6 group encoding"); this struct is what the loader
/// reconstructs per reading, not a fixed-width on-disk record.
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

/// Number of bits needed to represent the index values `0..num_values`
/// (i.e. `0..=num_values-1`). Returns 0 when there is at most one value,
/// in which case the index is always 0 and needs no bits on the wire.
#[inline]
pub fn bit_width(num_values: usize) -> u32 {
    if num_values <= 1 {
        0
    } else {
        (num_values as u64 - 1).ilog2() + 1
    }
}

/// Random-access read of the `index`-th `width`-bit value from a
/// contiguous LSB-first packed array beginning at byte `base` (matches
/// the layout [`BitWriter`] produces when values are written in order).
/// `width` 0 always yields 0. Bytes past the end read as 0.
#[inline]
pub fn read_packed_u32(data: &[u8], base: usize, index: usize, width: u32) -> u32 {
    if width == 0 {
        return 0;
    }
    let start_bit = index * width as usize;
    let mut byte = base + start_bit / 8;
    let mut bit_in_byte = (start_bit % 8) as u32;
    let mut acc: u64 = 0;
    let mut got: u32 = 0;
    while got < width {
        let b = (data.get(byte).copied().unwrap_or(0) as u64) >> bit_in_byte;
        acc |= b << got;
        got += 8 - bit_in_byte;
        bit_in_byte = 0;
        byte += 1;
    }
    let mask = if width == 32 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    };
    (acc & mask) as u32
}

/// LSB-first bit writer. The first bits written occupy the low-order
/// positions of each output byte; this matches [`BitReader`]. Call
/// [`BitWriter::align`] to flush a partial byte at a record boundary.
#[derive(Default)]
pub struct BitWriter {
    buf: Vec<u8>,
    acc: u64,
    nbits: u32,
}

impl BitWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append the low `width` bits of `value` (LSB first). `width` 0 is a
    /// no-op; `width` must be ≤ 32 and `value` must fit in `width` bits.
    #[inline]
    pub fn write(&mut self, value: u32, width: u32) {
        if width == 0 {
            return;
        }
        debug_assert!(width <= 32);
        debug_assert!(
            width == 32 || value < (1u32 << width),
            "value overflows width"
        );
        self.acc |= (value as u64) << self.nbits;
        self.nbits += width;
        while self.nbits >= 8 {
            self.buf.push((self.acc & 0xFF) as u8);
            self.acc >>= 8;
            self.nbits -= 8;
        }
    }

    /// Flush a partial byte so the next write starts byte-aligned.
    #[inline]
    pub fn align(&mut self) {
        if self.nbits > 0 {
            self.buf.push((self.acc & 0xFF) as u8);
            self.acc = 0;
            self.nbits = 0;
        }
    }

    /// Current byte length. Must be called on a byte boundary (i.e. right
    /// after [`BitWriter::new`] or [`BitWriter::align`]).
    #[inline]
    pub fn byte_len(&self) -> usize {
        debug_assert_eq!(self.nbits, 0, "byte_len called mid-byte");
        self.buf.len()
    }

    /// Finish, flushing any partial trailing byte.
    pub fn into_bytes(mut self) -> Vec<u8> {
        self.align();
        self.buf
    }
}

/// LSB-first bit reader over a byte slice, matching [`BitWriter`]. Bits
/// beyond the end of the slice read as 0 (so a truncated/corrupt index
/// yields recoverable zeros rather than a panic).
pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u64,
    nbits: u32,
}

impl<'a> BitReader<'a> {
    /// Begin reading at byte offset `start` (must be byte-aligned, as
    /// every surface's run is).
    #[inline]
    pub fn new(data: &'a [u8], start: usize) -> Self {
        Self {
            data,
            pos: start,
            acc: 0,
            nbits: 0,
        }
    }

    /// Read `width` bits (LSB first). `width` 0 returns 0.
    #[inline]
    pub fn read(&mut self, width: u32) -> u32 {
        if width == 0 {
            return 0;
        }
        debug_assert!(width <= 32);
        while self.nbits < width {
            let byte = self.data.get(self.pos).copied().unwrap_or(0);
            self.pos += 1;
            self.acc |= (byte as u64) << self.nbits;
            self.nbits += 8;
        }
        let mask = if width == 32 {
            u64::MAX
        } else {
            (1u64 << width) - 1
        };
        let v = (self.acc & mask) as u32;
        self.acc >>= width;
        self.nbits -= width;
        v
    }
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
    fn bit_width_matches_value_ranges() {
        assert_eq!(bit_width(0), 0);
        assert_eq!(bit_width(1), 0);
        assert_eq!(bit_width(2), 1);
        assert_eq!(bit_width(4), 2);
        assert_eq!(bit_width(5), 3);
        assert_eq!(bit_width(571), 10);
        assert_eq!(bit_width(165_998), 18);
    }

    #[test]
    fn bit_writer_reader_roundtrip_mixed_widths() {
        // Two byte-aligned "records" with assorted widths, including a
        // 0-width field (the single-lemma case) and a 1-bit flag.
        let mut w = BitWriter::new();
        w.write(0b101, 3);
        w.write(1, 1);
        w.write(300, 10);
        w.write(0, 0); // no-op
        w.align();
        let rec2_start = w.byte_len();
        w.write(165_997, 18);
        w.write(570, 10);
        let bytes = w.into_bytes();

        let mut r = BitReader::new(&bytes, 0);
        assert_eq!(r.read(3), 0b101);
        assert_eq!(r.read(1), 1);
        assert_eq!(r.read(10), 300);
        assert_eq!(r.read(0), 0);

        let mut r2 = BitReader::new(&bytes, rec2_start);
        assert_eq!(r2.read(18), 165_997);
        assert_eq!(r2.read(10), 570);
    }

    #[test]
    fn bit_reader_past_end_yields_zero() {
        let bytes = [0xFFu8];
        let mut r = BitReader::new(&bytes, 0);
        assert_eq!(r.read(8), 0xFF);
        assert_eq!(r.read(8), 0); // past end
    }

    #[test]
    fn read_packed_u32_random_access_matches_sequential_pack() {
        // Pack a sequence of values at a fixed width, with a leading
        // section of padding bytes to exercise a non-zero base.
        let width = 21u32;
        let values: Vec<u32> = (0..50).map(|i| (i * 40_000 + 7) % (1 << 21)).collect();
        let base = 3usize;
        let mut w = BitWriter::new();
        for v in &values {
            w.write(*v, width);
        }
        let packed = w.into_bytes();
        let mut data = vec![0xAAu8; base];
        data.extend_from_slice(&packed);
        for (i, v) in values.iter().enumerate() {
            assert_eq!(read_packed_u32(&data, base, i, width), *v, "index {i}");
        }
        // width 0 always reads 0.
        assert_eq!(read_packed_u32(&data, base, 5, 0), 0);
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
