//! Dump every surface's analysis SET in a canonical, order-independent
//! form, for verifying a format change is lossless. Prints one line per
//! surface: `surface\tA;A;A` with the analyses sorted. Diffing two dumps
//! (before/after a format change) proves the change preserved every
//! analysis of every surface.
//!
//! Run: `cargo run --release --example dump_analyses > dump.txt`

use std::error::Error;
use std::fs;
use std::io::{BufWriter, Write};

use de_morph::lexicon::Lexicon;
use fst::{Map as FstMap, Streamer};

fn main() -> Result<(), Box<dyn Error>> {
    let fst_bytes = fs::read("data/lexicon/lexicon.fst")?;
    let lex = Lexicon::open("data/lexicon/lexicon.fst", "data/lexicon/lexicon.dat")?;
    let map = FstMap::new(fst_bytes)?;

    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    let mut stream = map.stream();
    while let Some((key, _)) = stream.next() {
        let surface = std::str::from_utf8(key)?;
        let mut items: Vec<String> = lex
            .analyze(surface)
            .iter()
            .map(|a| format!("{}|{:?}|{:?}|{:?}", a.lemma, a.pos, a.features, a.source))
            .collect();
        items.sort();
        writeln!(out, "{}\t{}", surface, items.join(";"))?;
    }
    out.flush()?;
    Ok(())
}
