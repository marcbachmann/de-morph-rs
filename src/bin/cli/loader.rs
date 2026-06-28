//! Locate and load the runtime lexicon from disk.
//!
//! The `de-morph` binary embeds nothing — it reads the built lexicon
//! (`lexicon.fst` + `lexicon.dat`) from disk at startup. This keeps the
//! MIT binary free of CC BY-SA Wiktionary-derived bytes; the lexicon is a
//! separate, separately-licensed data artifact (build it with
//! `de-morph-build`, or fetch the packaged bundle).
//!
//! Paths default to `data/lexicon/lexicon.{fst,dat}` and can be overridden
//! with the `DE_MORPH_LEXICON_DIR` environment variable (pointing at a
//! directory that holds `lexicon.fst` + `lexicon.dat`).

use std::error::Error;
use std::path::PathBuf;

use de_morph::{Analyzer, Lexicon};

const DEFAULT_DIR: &str = "data/lexicon";

/// Resolve the `(fst, dat)` paths from `DE_MORPH_LEXICON_DIR` or the default.
pub fn paths() -> (PathBuf, PathBuf) {
    let dir = std::env::var("DE_MORPH_LEXICON_DIR").unwrap_or_else(|_| DEFAULT_DIR.to_string());
    let dir = PathBuf::from(dir);
    (dir.join("lexicon.fst"), dir.join("lexicon.dat"))
}

/// Read the raw FST + side-table bytes from disk (for tools that need the
/// FST bytes directly, e.g. surface streaming / benchmarks).
pub fn read_bytes() -> Result<(Vec<u8>, Vec<u8>), Box<dyn Error>> {
    let (fst, dat) = paths();
    let fst_bytes = std::fs::read(&fst).map_err(|e| missing(&fst, e))?;
    let dat_bytes = std::fs::read(&dat).map_err(|e| missing(&dat, e))?;
    Ok((fst_bytes, dat_bytes))
}

/// Load the lexicon from disk. Errors (with a build hint) if absent —
/// use this for subcommands that are meaningless without a lexicon.
pub fn lexicon() -> Result<Lexicon, Box<dyn Error>> {
    let (fst, dat) = paths();
    Lexicon::open(&fst, &dat).map_err(|e| missing(&fst, e))
}

/// Build an analyzer over the on-disk lexicon (strict — errors if absent).
pub fn analyzer() -> Result<Analyzer, Box<dyn Error>> {
    Ok(Analyzer::from_lexicon(lexicon()?))
}

/// Build an analyzer, falling back to an empty (OOV-only) analyzer with a
/// warning when no lexicon is present. Used by `analyze`, which is still
/// useful — if degraded — without a lexicon.
pub fn analyzer_or_empty() -> Analyzer {
    match lexicon() {
        Ok(lex) => Analyzer::from_lexicon(lex),
        Err(e) => {
            eprintln!(
                "warning: {e}\n  continuing with OOV-only analysis (no lexicon). \
                 Build one with `de-morph-build all`."
            );
            Analyzer::empty()
        }
    }
}

fn missing(path: &std::path::Path, e: impl std::fmt::Display) -> Box<dyn Error> {
    format!(
        "could not load lexicon at {}: {e}\n  \
         build it with `de-morph-build all`, or set DE_MORPH_LEXICON_DIR.",
        path.display()
    )
    .into()
}
