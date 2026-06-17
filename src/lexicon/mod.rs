//! Compact, self-describing on-disk format for the morphological
//! lexicon plus a runtime loader and analyzer.
//!
//! Two artefacts ship together:
//!
//! - **FST file** (`*.fst`) — a `fst::Map<bytes, u64>` where the key is
//!   the surface form (UTF-8) and the value is a packed pointer into
//!   the side table:
//!
//!   ```text
//!   value = (count as u64) << 32 | (offset as u64)
//!   ```
//!
//!   `count` is the number of analyses for this surface; `offset` is
//!   the byte offset into the side table's analyses array.
//!
//! - **Side table** (`*.dat`) — a binary blob with a fixed-size header,
//!   a lemma intern table, and a packed array of 12-byte
//!   `AnalysisRecord`s. See [`format`] for the exact layout.
//!
//! The runtime [`Lexicon`] reads both files into memory (a few tens of
//! MiB) and exposes [`Lexicon::analyze`] for O(|surface|) lookups.
//!
//! For the build direction see [`build`].

pub mod build;
pub mod format;
pub mod load;

pub use build::{BuildError, LexiconBuilder};
pub use format::{AnalysisRecord, HEADER_SIZE, MAGIC};
pub use load::{LoadError, Lexicon};
