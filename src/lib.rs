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
//! Pre-alpha. The runtime API surface is a stub; no analysis data is bundled
//! yet.

pub mod analysis;
pub mod analyzer;
pub mod lexicon;
pub mod paradigm;

#[cfg(feature = "extractor")]
pub mod wiktionary;

pub use analysis::{Analysis, Features, UPOS, Source};
pub use analyzer::Analyzer;
pub use lexicon::{Lexicon, LexiconBuilder};
