//! Extract German adjective analyses (full paradigm) from a Wiktionary dump.
//!
//! Output is JSON-lines with the union schema used by `extract-nouns`
//! and `extract-verbs`. Adjective rows fill `degree`, `declension`,
//! `case`, `number`, and `gender`; the bare predicative form leaves
//! declension/case/number/gender empty.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use serde::Serialize;

use de_morph::analysis::{Case, Declension, Degree, Gender, Number, UPOS, Source};
use de_morph::wiktionary::adjective::extract_adjectives;
use de_morph::wiktionary::dump::PageReader;
use de_morph::wiktionary::ExtractedEntry;

const DEFAULT_INPUT: &str = "data/wiktionary/raw/dewiktionary-20260601-pages-articles.xml.bz2";
const DEFAULT_OUTPUT: &str = "data/wiktionary/processed/adjectives.jsonl";
const PROGRESS_EVERY: u64 = 100_000;

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
    degree: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    declension: Option<&'static str>,
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
        degree: e.features.degree.map(degree_str),
        declension: e.features.declension.map(declension_str),
        source: source_str(e.source),
        source_title: &e.source_title,
    }
}

fn pos_str(p: UPOS) -> &'static str {
    match p {
        UPOS::NOUN => "Noun",
        UPOS::VERB => "Verb",
        UPOS::ADJ => "Adj",
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

fn degree_str(d: Degree) -> &'static str {
    match d {
        Degree::Pos => "Pos",
        Degree::Cmp => "Cmp",
        Degree::Sup => "Sup",
    }
}

fn declension_str(d: Declension) -> &'static str {
    match d {
        Declension::Strong => "Strong",
        Declension::Weak => "Weak",
        Declension::Mixed => "Mixed",
    }
}

fn source_str(s: Source) -> &'static str {
    match s {
        Source::Lexicon => "Lexicon",
        Source::Generated => "Generated",
        Source::Guessed => "Guessed",
    }
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
                eprintln!("extract-adjectives [--input PATH] [--output PATH] [--limit N]");
                std::process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
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

fn main() -> Result<()> {
    let args = parse_args()?;
    eprintln!("Reading {}", args.input.display());
    eprintln!("Writing {}", args.output.display());

    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let out_file = File::create(&args.output)?;
    let mut writer = BufWriter::with_capacity(1 << 20, out_file);

    let reader = PageReader::open_bz2(&args.input)?;

    let start = Instant::now();
    let mut pages_total: u64 = 0;
    let mut pages_main: u64 = 0;
    let mut entries: u64 = 0;

    for page in reader {
        let page = page?;
        pages_total += 1;
        if !page.is_main_namespace() {
            continue;
        }
        pages_main += 1;

        for entry in extract_adjectives(&page.title, &page.text) {
            serde_json::to_writer(&mut writer, &entry_to_record(&entry))?;
            writer.write_all(b"\n")?;
            entries += 1;
        }

        if pages_main % PROGRESS_EVERY == 0 {
            eprintln!(
                "  pages_total={pages_total} pages_main={pages_main} entries={entries} elapsed={:.1}s",
                start.elapsed().as_secs_f64()
            );
        }
        if let Some(limit) = args.limit {
            if pages_main >= limit {
                break;
            }
        }
    }

    writer.flush()?;
    let elapsed = start.elapsed().as_secs_f64();
    eprintln!("Done.");
    eprintln!("  pages_total = {pages_total}");
    eprintln!("  pages_main  = {pages_main}");
    eprintln!("  entries     = {entries}");
    eprintln!("  elapsed     = {elapsed:.1}s");
    eprintln!(
        "  rate        = {:.0} pages/s",
        pages_total as f64 / elapsed.max(0.001)
    );

    Ok(())
}
