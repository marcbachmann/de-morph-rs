//! Extract German closed-class pronoun / determiner analyses from a
//! Wiktionary dump.
//!
//! Mines the two structured flexion templates (`{{Pronomina-Tabelle}}`,
//! `{{Deutsch Pronomen Übersicht}}`) plus the no-table invariants (the
//! indeclinable `-lei` family: `allerlei`, `vielerlei`, …). Lemmas already
//! supplied by the hand-curated closed-class table
//! (`src/paradigm/closed_class.rs`) are skipped, so this only *adds* coverage.
//!
//! Output is JSON-lines, one record per (surface, analysis) cell, matching the
//! schema `build-lexicon` ingests (includes `pron_type`). Each record carries
//! the source Wiktionary title for CC BY-SA 4.0 attribution.
//!
//! References (verified):
//! - JSON-lines: <https://jsonlines.org/>
//! - CC BY-SA 4.0 attribution: <https://creativecommons.org/licenses/by-sa/4.0/legalcode> § 3.a.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use de_morph::analysis::{Case, Gender, Number, PronType, Source, UPOS};
use de_morph::paradigm::generate_closed_class_entries;
use de_morph::wiktionary::dump::PageReader;
use de_morph::wiktionary::pronoun::extract_pronouns;
use de_morph::wiktionary::ExtractedEntry;
use serde::Serialize;

const DEFAULT_INPUT: &str = "data/wiktionary/raw/dewiktionary-20260601-pages-articles.xml.bz2";
const DEFAULT_OUTPUT: &str = "data/wiktionary/processed/pronouns.jsonl";
const PROGRESS_EVERY: u64 = 200_000;

#[derive(Serialize)]
struct OutputRecord<'a> {
    surface: &'a str,
    lemma: &'a str,
    pos: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    gender: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    number: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    case: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pron_type: Option<&'static str>,
    source: &'static str,
    source_title: &'a str,
}

fn entry_to_record(e: &ExtractedEntry) -> OutputRecord<'_> {
    OutputRecord {
        surface: &e.surface,
        lemma: &e.lemma,
        pos: pos_str(e.pos),
        gender: e.features.gender.map(gender_str),
        number: e.features.number.map(number_str),
        case: e.features.case.map(case_str),
        pron_type: e.features.pron_type.map(pron_type_str),
        source: source_str(e.source),
        source_title: &e.source_title,
    }
}

fn source_str(s: Source) -> &'static str {
    match s {
        Source::Attested => "Attested",
        Source::Inflected => "Inflected",
        Source::Composed => "Composed",
        Source::Predicted => "Predicted",
    }
}

fn pos_str(p: UPOS) -> &'static str {
    match p {
        UPOS::DET => "Det",
        UPOS::PRON => "Pron",
        _ => "X",
    }
}

fn gender_str(g: Gender) -> &'static str {
    match g {
        Gender::Masc => "Masc",
        Gender::Fem => "Fem",
        Gender::Neut => "Neut",
    }
}

fn number_str(n: Number) -> &'static str {
    match n {
        Number::Sg => "Sg",
        Number::Pl => "Pl",
    }
}

fn case_str(c: Case) -> &'static str {
    match c {
        Case::Nom => "Nom",
        Case::Gen => "Gen",
        Case::Dat => "Dat",
        Case::Acc => "Acc",
    }
}

fn pron_type_str(p: PronType) -> &'static str {
    match p {
        PronType::Prs => "Prs",
        PronType::Refl => "Refl",
        PronType::Rel => "Rel",
        PronType::Int => "Int",
        PronType::Dem => "Dem",
        PronType::Ind => "Ind",
        PronType::Neg => "Neg",
        PronType::Art => "Art",
    }
}

/// Lemmas already produced by the hand-curated closed-class table. The
/// extractor skips these so it never conflicts with or duplicates the
/// maintainer-owned paradigms.
fn covered_lemmas() -> HashSet<String> {
    generate_closed_class_entries()
        .into_iter()
        .map(|(_, a)| a.lemma.into_owned())
        .collect()
}

struct Args {
    input: PathBuf,
    output: PathBuf,
    limit: Option<u64>,
}

fn parse_args() -> Result<Args> {
    let mut input = PathBuf::from(DEFAULT_INPUT);
    let mut output = PathBuf::from(DEFAULT_OUTPUT);
    let mut limit = None;

    let argv: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--input" => {
                i += 1;
                input = argv.get(i).context("--input requires a value")?.into();
            }
            "--output" => {
                i += 1;
                output = argv.get(i).context("--output requires a value")?.into();
            }
            "--limit" => {
                i += 1;
                limit = Some(
                    argv.get(i)
                        .context("--limit requires a value")?
                        .parse::<u64>()
                        .context("--limit must be an integer")?,
                );
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                print_usage();
                std::process::exit(2);
            }
        }
        i += 1;
    }
    Ok(Args {
        input,
        output,
        limit,
    })
}

fn print_usage() {
    eprintln!(
        "extract-pronouns — extract German pronoun/determiner analyses from a Wiktionary dump\n\
\n\
Usage:\n\
  cargo run --release --features extractor --bin extract-pronouns -- [options]\n\
\n\
Options:\n\
  --input <PATH>    Input .xml.bz2 dump file\n\
                    (default: {DEFAULT_INPUT})\n\
  --output <PATH>   Output .jsonl file\n\
                    (default: {DEFAULT_OUTPUT})\n\
  --limit <N>       Stop after processing N main-namespace pages\n\
  --help            Show this message and exit."
    );
}

fn main() -> Result<()> {
    let args = parse_args()?;

    eprintln!("Reading {}", args.input.display());
    eprintln!("Writing {}", args.output.display());

    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent).context("creating output directory")?;
    }

    let covered = covered_lemmas();
    eprintln!("Excluding {} hand-curated closed-class lemmas", covered.len());

    let out_file = File::create(&args.output).context("opening output file")?;
    let mut writer = BufWriter::with_capacity(1 << 20, out_file);

    let reader = PageReader::open_bz2(&args.input)
        .with_context(|| format!("opening dump at {}", args.input.display()))?;

    let start = Instant::now();
    let mut pages_total: u64 = 0;
    let mut pages_main: u64 = 0;
    let mut entries: u64 = 0;
    let mut lemmas: HashSet<String> = HashSet::new();
    let mut errors: u64 = 0;

    for page in reader {
        let page = match page {
            Ok(p) => p,
            Err(e) => {
                eprintln!("xml read error: {e}");
                errors += 1;
                continue;
            }
        };
        pages_total += 1;
        if !page.is_main_namespace() {
            continue;
        }
        pages_main += 1;

        for entry in extract_pronouns(&page.title, &page.text, &covered) {
            let record = entry_to_record(&entry);
            serde_json::to_writer(&mut writer, &record).context("write json")?;
            writer.write_all(b"\n").context("newline")?;
            entries += 1;
            lemmas.insert(entry.lemma);
        }

        if pages_main % PROGRESS_EVERY == 0 {
            eprintln!(
                "  pages_main={pages_main} entries={entries} lemmas={} elapsed={:.1}s",
                lemmas.len(),
                start.elapsed().as_secs_f64()
            );
        }

        if let Some(limit) = args.limit {
            if pages_main >= limit {
                eprintln!("  --limit reached, stopping");
                break;
            }
        }
    }

    writer.flush().context("flushing output")?;

    let elapsed = start.elapsed().as_secs_f64();
    eprintln!("Done.");
    eprintln!("  pages_total = {pages_total}");
    eprintln!("  pages_main  = {pages_main}");
    eprintln!("  entries     = {entries}");
    eprintln!("  new lemmas  = {}", lemmas.len());
    eprintln!("  errors      = {errors}");
    eprintln!("  elapsed     = {elapsed:.1}s");

    Ok(())
}
