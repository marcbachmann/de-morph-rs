//! Demonstrate the runtime analyzer on the built lexicon.
//!
//! The lexicon is embedded via `include_bytes!`, so `data/lexicon/lexicon.
//! {fst,dat}` must exist **at compile time**; build it first with
//! `cargo run --release --features extractor --bin build-lexicon`. Loading
//! is then zero-copy (see `LEXICON_FST`/`LEXICON_DAT` below).
//!
//! Usage:
//!
//!   # Built-in curated showcase (nouns, verbs, adjectives, OOV):
//!   cargo run --release --example analyze_demo
//!
//!   # Analyze each word of a sentence passed as an argument:
//!   cargo run --release --example analyze_demo -- "Ich gehe heute zur Schule."
//!
//!   # Pipe input — each line is treated as one sentence:
//!   echo "Ich gehe heute zur Schule." | cargo run --release --example analyze_demo
//!   cat sentences.txt | cargo run --release --example analyze_demo
//!
//!   # Force stdin read with the `-` argument (overrides showcase even
//!   # when stdin is a TTY):
//!   cargo run --release --example analyze_demo -- -
//!
//!   # Enable the Swiss ss→ß orthography bridge (works with any input mode):
//!   cargo run --release --example analyze_demo -- --swiss "Das ist die Strasse durch Zürich."

use std::io::{self, BufRead, IsTerminal};
use std::time::Instant;

use de_morph::{Analysis, Analyzer, Lexicon, Source};

/// The lexicon, embedded into the binary at compile time. `include_bytes!`
/// places these in the executable's read-only data, so at runtime they are
/// `&'static [u8]` — demand-paged from the binary image, never read into the
/// process heap. Paired with [`Lexicon::from_static`], lemma lookups borrow
/// straight out of these bytes (zero-copy), so no per-analysis allocation and
/// no resident copy of the ~8 MiB dictionary.
///
/// Requires `data/lexicon/lexicon.{fst,dat}` to exist *at compile time* — see
/// the module docs for how to build them.
static LEXICON_FST: &[u8] = include_bytes!("../data/lexicon/lexicon.fst");
static LEXICON_DAT: &[u8] = include_bytes!("../data/lexicon/lexicon.dat");

const SAMPLES: &[&str] = &[
    // Noun forms
    "Tisch",   // expected: Nom/Dat/Acc Sg masc
    "Tisches", // expected: Gen Sg masc
    "Tischen", // expected: Dat Pl masc
    "Frauen",  // expected: all four Pl cases of Frau
    "Bücher",  // expected: Nom/Gen/Acc Pl neut of Buch
    "Büchern", // expected: Dat Pl neut of Buch
    // Verb forms
    "lieben",   // expected: Inf + 1/3 Pl Pres Ind + 1 Pl Konj I + 3 Pl Konj I
    "liebte",   // expected: 1/3 Sg Past Ind + 1/3 Sg Konj II
    "liebtest", // expected: 2 Sg Past Ind + 2 Sg Konj II
    "geliebt",  // expected: PtcPerf
    "war",      // expected: 1/3 Sg Past Ind of sein
    // Adjective forms
    "groß",   // expected: predicative Pos
    "größer", // expected: predicative Cmp; AND Sg Nom Masc Strong of comparative
    "großen", // expected: many cells (Dat Pl, Acc Sg Masc, Sg Gen/Dat M+N, ...)
    "größte", // expected: Sup attributive
    // OOV — these are unlikely to be in Wiktionary
    "Quitschung",   // expected: Predicted (-ung → Fem Strong)
    "Quitschungen", // OOV Dat Pl recovery via suffix-strip
    "Quitschen",    // OOV Dat Pl recovery: lemma=Quitsch, Strong Masc
    "Schmurkes",    // OOV Gen Sg recovery: lemma=Schmurk, Strong Masc
    "xyzzy",        // no suffix → low-confidence fallback
];

/// What to analyse, decided from CLI args + whether stdin is piped.
enum Mode {
    /// Built-in curated SAMPLES showcase.
    Showcase,
    /// A sentence passed as a CLI argument (positional args joined).
    Sentence(String),
    /// Read newline-delimited sentences from stdin. Triggered by piped
    /// stdin (non-TTY) with no positional arg, or by the explicit `-`
    /// argument.
    Stdin,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse CLI: optional --swiss flag plus optional positional
    // sentence (the rest of argv joined with spaces) or a literal `-`
    // to force stdin reads.
    let mut swiss = false;
    let mut oov = true;
    let mut force_stdin = false;
    let mut positional: Vec<String> = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--swiss" => swiss = true,
            "--no-oov" => oov = false,
            "-" => force_stdin = true,
            "--help" | "-h" => {
                eprintln!("usage: analyze_demo [--swiss] [--no-oov] [SENTENCE | -]");
                eprintln!("  (none)        → curated showcase");
                eprintln!("  SENTENCE      → tokenise on whitespace, analyse each word");
                eprintln!("  -             → force read from stdin (one sentence per line)");
                eprintln!("  pipe to stdin → same as `-`, auto-detected when stdin is not a TTY");
                eprintln!("  --swiss       → enable ss→ß orthography bridge");
                eprintln!("  --no-oov      → disable out-of-vocabulary guessing (drops Predicted results)");
                std::process::exit(0);
            }
            _ => positional.push(arg),
        }
    }

    let mode = if !positional.is_empty() {
        Mode::Sentence(positional.join(" "))
    } else if force_stdin || !io::stdin().is_terminal() {
        Mode::Stdin
    } else {
        Mode::Showcase
    };

    eprintln!("Loading embedded lexicon (zero-copy)...");
    let load_start = Instant::now();
    let mut analyzer = Analyzer::from_lexicon(Lexicon::from_static(LEXICON_FST, LEXICON_DAT)?);
    if swiss {
        analyzer = analyzer.with_swiss_orthography(true);
    }
    if !oov {
        analyzer = analyzer.with_oov_fallback(false);
    }
    eprintln!("  loaded in {:.2}s\n", load_start.elapsed().as_secs_f64());

    match mode {
        Mode::Showcase => analyze_showcase(&analyzer),
        Mode::Sentence(s) => analyze_sentence(&analyzer, &s),
        Mode::Stdin => analyze_stdin(&analyzer)?,
    }
    Ok(())
}

/// Read sentences line-by-line from stdin and analyse each.
fn analyze_stdin(analyzer: &Analyzer) -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let handle = stdin.lock();
    for line in handle.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        analyze_sentence(analyzer, trimmed);
    }
    Ok(())
}

fn analyze_showcase(analyzer: &Analyzer) {
    for surface in SAMPLES {
        print_one(analyzer, surface);
    }
}

/// Tokenise `sentence` on whitespace and analyse each token. Trailing
/// punctuation (.,;:!?…) is stripped so a token like `Schule.` is
/// analysed as `Schule`. Hyphens and apostrophes are preserved so
/// hyphenated compounds and contractions stay intact.
fn analyze_sentence(analyzer: &Analyzer, sentence: &str) {
    println!("Input: {sentence:?}\n");
    for raw in sentence.split_whitespace() {
        let token = raw.trim_matches(|c: char| c.is_ascii_punctuation() && c != '-' && c != '\'');
        if token.is_empty() {
            continue;
        }
        print_one(analyzer, token);
    }
}

fn print_one(analyzer: &Analyzer, surface: &str) {
    println!("==== {surface} ====");
    let t = Instant::now();
    let analyses = analyzer.analyze(surface);
    let elapsed_us = t.elapsed().as_micros();
    if analyses.is_empty() {
        println!("  (no analysis)");
    }
    for a in &analyses {
        println!(
            "  {:<10} {:?}{}{}{}{}{}{}{}{}  · {} [{}]",
            a.lemma,
            a.pos,
            opt(a.features.gender),
            opt(a.features.number),
            opt(a.features.case),
            opt(a.features.person),
            opt(a.features.tense),
            opt(a.features.mood),
            opt(a.features.form),
            opt(a.features.aux),
            provenance(a),
            confidence(a.source),
        );
    }
    println!("  ({} result(s) in {} µs)\n", analyses.len(), elapsed_us);
}

/// Human-readable provenance for one analysis, naming the lemma it came
/// from. This is NOT a probability — the analyzer carries no corpus
/// frequencies — but a trust tier derived from how the form was obtained.
fn provenance(a: &Analysis) -> String {
    match a.source {
        Source::Attested => "attested in lexicon".to_string(),
        Source::Inflected => format!("inflected from lemma «{}»", a.lemma),
        Source::Composed => "composed from in-lexicon parts".to_string(),
        Source::Predicted => "predicted — lemma not in lexicon".to_string(),
    }
}

/// Confidence tier derived from the source. Ordered Attested > Inflected
/// > Composed > Predicted.
fn confidence(s: Source) -> &'static str {
    match s {
        Source::Attested => "confidence: high",
        Source::Inflected => "confidence: medium",
        Source::Composed => "confidence: medium-low",
        Source::Predicted => "confidence: low",
    }
}

fn opt<T: std::fmt::Debug>(v: Option<T>) -> String {
    match v {
        Some(x) => format!(" {x:?}"),
        None => String::new(),
    }
}
