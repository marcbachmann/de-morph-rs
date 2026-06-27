//! Dump every surface's analysis SET in a canonical, order-independent
//! form, for verifying a format change is lossless. Prints one line per
//! surface: `surface\tA;A;A` with the analyses sorted. Diffing two dumps
//! (before/after a format change) proves the change preserved every
//! analysis of every surface.
//!
//! Run: `de-morph dump > dump.txt`

use std::error::Error;
use std::fmt::Write as _;
use std::io::{BufWriter, Write};

use de_morph::analysis::Features;
use fst::{Map as FstMap, Streamer};

use crate::loader::LEXICON_FST;

pub fn run(_args: &[String]) -> Result<(), Box<dyn Error>> {
    let lex = crate::loader::lexicon()?;
    let map = FstMap::new(LEXICON_FST)?;

    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    let mut stream = map.stream();
    while let Some((key, _)) = stream.next() {
        let surface = std::str::from_utf8(key)?;
        let mut items: Vec<String> = lex
            .analyze(surface)
            .iter()
            .map(|a| {
                format!(
                    "{}|{:?}|{}|{:?}",
                    a.lemma,
                    a.pos,
                    fmt_features(&a.features),
                    a.source
                )
            })
            .collect();
        items.sort();
        writeln!(out, "{}\t{}", surface, items.join(";"))?;
    }
    out.flush()?;
    Ok(())
}

/// Compact, canonical feature rendering: only set fields, `key=Val`,
/// space-separated, in a fixed order. Empty when no feature is set.
/// Keeping the keys (vs. bare values) keeps the dump lossless — every
/// distinct feature combination maps to a distinct string.
fn fmt_features(f: &Features) -> String {
    let mut s = String::new();
    let mut push = |k: &str, v: String| {
        if !s.is_empty() {
            s.push(' ');
        }
        let _ = write!(s, "{k}={v}");
    };
    if let Some(x) = f.case {
        push("case", format!("{x:?}"));
    }
    if let Some(x) = f.number {
        push("num", format!("{x:?}"));
    }
    if let Some(x) = f.gender {
        push("gen", format!("{x:?}"));
    }
    if let Some(x) = f.person {
        push("pers", format!("{x:?}"));
    }
    if let Some(x) = f.tense {
        push("tense", format!("{x:?}"));
    }
    if let Some(x) = f.mood {
        push("mood", format!("{x:?}"));
    }
    if let Some(x) = f.voice {
        push("voice", format!("{x:?}"));
    }
    if let Some(x) = f.form {
        push("form", format!("{x:?}"));
    }
    if let Some(x) = f.degree {
        push("deg", format!("{x:?}"));
    }
    if let Some(x) = f.declension {
        push("decl", format!("{x:?}"));
    }
    if let Some(x) = f.pron_type {
        push("pron", format!("{x:?}"));
    }
    if let Some(x) = f.poss_person {
        push("posspers", format!("{x:?}"));
    }
    if let Some(x) = f.poss_number {
        push("possnum", format!("{x:?}"));
    }
    if let Some(x) = f.aux {
        push("aux", format!("{x:?}"));
    }
    s
}
