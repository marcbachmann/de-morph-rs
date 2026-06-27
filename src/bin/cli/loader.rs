//! Shared lexicon loading for the `de-morph` subcommands.
//!
//! The lexicon is embedded via `include_bytes!`, so `data/lexicon/lexicon.
//! {fst,dat}` must exist **at compile time**; build it first with
//! `cargo run --release --features extractor --bin build-lexicon`. Loading
//! is then zero-copy ([`Lexicon::from_static`]) — the side table stays
//! demand-paged in the binary image, never copied into the process heap.
use de_morph::{Analyzer, Lexicon};

/// The lexicon FST + side table, embedded into the binary at compile time.
/// `include_bytes!` places these in read-only data, so at runtime they are
/// `&'static [u8]` paired with [`Lexicon::from_static`] for zero-copy lookup.
pub static LEXICON_FST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/lexicon.fst"));
pub static LEXICON_DAT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/lexicon.dat"));

/// Open the embedded lexicon (zero-copy).
pub fn lexicon() -> Result<Lexicon, Box<dyn std::error::Error>> {
    Ok(Lexicon::from_static(LEXICON_FST, LEXICON_DAT)?)
}

/// Build an analyzer over the embedded lexicon.
pub fn analyzer() -> Result<Analyzer, Box<dyn std::error::Error>> {
    Ok(Analyzer::from_lexicon(lexicon()?))
}
