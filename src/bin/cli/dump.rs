//! Dump every surface's analysis SET in a canonical, order-independent
//! form, for verifying a format change is lossless. Prints one line per
//! surface: `surface\tA;A;A` with the analyses sorted. Diffing two dumps
//! (before/after a format change) proves the change preserved every
//! analysis of every surface. The rendering itself lives in the library
//! ([`de_morph::Lexicon::write_canonical_dump`]) so `de-morph-build` can
//! hash the identical output as the lossless fingerprint.
//!
//! Run: `de-morph dump > dump.txt`

use std::error::Error;
use std::io::{BufWriter, Write};

pub fn run(_args: &[String]) -> Result<(), Box<dyn Error>> {
    let lex = crate::loader::lexicon()?;
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    lex.write_canonical_dump(&mut out)?;
    out.flush()?;
    Ok(())
}
