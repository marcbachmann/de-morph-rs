//! German morphological analyzer based on finite-state transducers.
//!
//! # Licensing
//!
//! This crate's source code is licensed under MIT (see `LICENSE-MIT`). Data
//! files distributed with or referenced by this crate may carry separate
//! licenses; the `NOTICE` file enumerates all third-party attributions and
//! verbatim license texts live in `LICENSES/`.
//!
//! # Status
//!
//! Pre-alpha. The analyzer is implemented — FST-backed lexicon lookup
//! with Swiss-orthography, hyphenated-compound, and OOV-guessing
//! fallbacks — but the published crate bundles no data and the API and
//! on-disk formats may still change.

pub mod analysis;
pub mod analyzer;
pub mod lexicon;
pub mod paradigm;

pub use analysis::{Analysis, Features, Source, UPOS};
pub use analyzer::Analyzer;
pub use lexicon::{Lexicon, LexiconBuilder};
