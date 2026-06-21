//! Build the runtime lexicon (`lexicon.fst` + `lexicon.dat`) from one
//! or more JSONL inputs produced by `extract-nouns` / `extract-verbs`.
//!
//! Output is a self-contained pair of files: the FST holds the surface
//! → packed-u64-pointer mapping; the .dat file holds the lemma intern
//! table plus the packed `AnalysisRecord` array. See
//! `src/lexicon/format.rs` for the layout.

use std::collections::HashMap;
use std::fs::{metadata, File};
use std::io::{BufRead, BufReader, BufWriter};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use de_morph::analysis::{
    Aux, Case, Degree, Features, Gender, Mood, Number, Person, PronType, Source, Tense, VerbForm,
    UPOS,
};
use de_morph::lexicon::{is_clean_surface, LexiconBuilder};
use de_morph::paradigm::generate_closed_class_entries;
use serde::Deserialize;

#[derive(Deserialize)]
struct Record {
    surface: String,
    lemma: String,
    pos: String,
    #[serde(default)]
    gender: Option<String>,
    #[serde(default)]
    number: Option<String>,
    #[serde(default)]
    case: Option<String>,
    #[serde(default)]
    person: Option<String>,
    #[serde(default)]
    tense: Option<String>,
    #[serde(default)]
    mood: Option<String>,
    #[serde(default)]
    form: Option<String>,
    #[serde(default)]
    degree: Option<String>,
    #[serde(default)]
    pron_type: Option<String>,
    #[serde(default)]
    aux: Option<String>,
    #[serde(default)]
    source: Option<String>,
}

struct Counters {
    parsed: u64,
    skipped_unknown_field: u64,
    skipped_contaminated: u64,
    by_pos: HashMap<&'static str, u64>,
    by_source: HashMap<&'static str, u64>,
}

fn main() -> Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    let args = parse_args(&argv)?;

    let start = Instant::now();
    let mut builder = LexiconBuilder::new();
    let mut counters = Counters {
        parsed: 0,
        skipped_unknown_field: 0,
        skipped_contaminated: 0,
        by_pos: HashMap::new(),
        by_source: HashMap::new(),
    };

    for input in &args.inputs {
        // Skip missing inputs (e.g. adverbs.jsonl hasn't been built yet).
        if !input.exists() {
            eprintln!("Skipping {} (not present)", input.display());
            continue;
        }
        eprintln!("Reading {}", input.display());
        ingest_file(input, &mut builder, &mut counters)
            .with_context(|| format!("ingesting {}", input.display()))?;
    }

    // Add hard-coded closed-class entries (personal pronouns, articles,
    // negation determiner). These don't come from JSONL — they're
    // generated directly from the paradigm tables.
    eprintln!("Adding closed-class entries (pronouns + articles)");
    let cc_before = counters.parsed;
    for (surface, analysis) in generate_closed_class_entries() {
        builder
            .add(
                &surface,
                &analysis.lemma,
                analysis.pos,
                analysis.features,
                analysis.source,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        counters.parsed += 1;
        *counters.by_pos.entry(pos_label(analysis.pos)).or_insert(0) += 1;
        *counters
            .by_source
            .entry(source_label(analysis.source))
            .or_insert(0) += 1;
    }
    eprintln!(
        "  added {} closed-class entries",
        counters.parsed - cc_before
    );

    eprintln!(
        "Ingested {} records across {} files + closed-class in {:.1}s",
        counters.parsed,
        args.inputs.len(),
        start.elapsed().as_secs_f64()
    );
    if counters.skipped_unknown_field > 0 {
        eprintln!(
            "  ({} records had unrecognised enum values and were skipped)",
            counters.skipped_unknown_field
        );
    }
    if counters.skipped_contaminated > 0 {
        eprintln!(
            "  ({} surfaces dropped as contaminated markup/whitespace)",
            counters.skipped_contaminated
        );
    }

    if let Some(parent) = args.fst_out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = args.dat_out.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let fst_file = BufWriter::new(File::create(&args.fst_out)?);
    let dat_file = BufWriter::new(File::create(&args.dat_out)?);
    let stats = builder.finish(fst_file, dat_file)?;

    let fst_size = metadata(&args.fst_out)?.len();
    let dat_size = metadata(&args.dat_out)?.len();

    eprintln!();
    eprintln!("=== Build complete ===");
    eprintln!("  Surfaces:        {:>10}", stats.num_surfaces);
    eprintln!("  Lemmas:          {:>10}", stats.num_lemmas);
    eprintln!("  Analyses:        {:>10}", stats.num_analyses);
    eprintln!("  Total records:   {:>10}", stats.total_records);
    eprintln!(
        "  FST file:        {:>10}  ({})",
        humanize(fst_size),
        args.fst_out.display()
    );
    eprintln!(
        "  Side table:      {:>10}  ({})",
        humanize(dat_size),
        args.dat_out.display()
    );
    eprintln!("  Combined:        {:>10}", humanize(fst_size + dat_size));
    eprintln!();
    eprintln!("By POS:");
    let mut pos_pairs: Vec<_> = counters.by_pos.iter().collect();
    pos_pairs.sort_by_key(|(_, v)| std::cmp::Reverse(**v));
    for (k, v) in pos_pairs {
        eprintln!("  {k:<6} {v:>10}");
    }
    eprintln!("By source:");
    let mut src_pairs: Vec<_> = counters.by_source.iter().collect();
    src_pairs.sort_by_key(|(_, v)| std::cmp::Reverse(**v));
    for (k, v) in src_pairs {
        eprintln!("  {k:<10} {v:>10}");
    }
    eprintln!();
    eprintln!("Elapsed: {:.1}s", start.elapsed().as_secs_f64());

    Ok(())
}

fn ingest_file(
    path: &PathBuf,
    builder: &mut LexiconBuilder,
    counters: &mut Counters,
) -> Result<()> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    for (lineno, line) in reader.lines().enumerate() {
        let line = line?;
        let rec: Record =
            serde_json::from_str(&line).with_context(|| format!("line {}", lineno + 1))?;

        // Drop surfaces contaminated by leaked wikitext/HTML markup or
        // control whitespace (HTML comments, <small> tags, embedded
        // newlines, doubled braces, double spaces). These are extraction
        // artifacts, not German words, and must never enter the FST.
        if !is_clean_surface(&rec.surface) {
            counters.skipped_contaminated += 1;
            continue;
        }

        let pos = match parse_pos(&rec.pos) {
            Some(p) => p,
            None => {
                counters.skipped_unknown_field += 1;
                continue;
            }
        };
        let features = build_features(&rec, counters);
        let source = parse_source(rec.source.as_deref()).unwrap_or(Source::Lexicon);

        *counters.by_pos.entry(pos_label(pos)).or_insert(0) += 1;
        *counters.by_source.entry(source_label(source)).or_insert(0) += 1;
        counters.parsed += 1;

        builder
            .add(&rec.surface, &rec.lemma, pos, features, source)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    Ok(())
}

fn build_features(rec: &Record, counters: &mut Counters) -> Features {
    let mut f = Features::empty();
    if let Some(s) = &rec.gender {
        match s.as_str() {
            "Masc" => f.gender = Some(Gender::Masc),
            "Fem" => f.gender = Some(Gender::Fem),
            "Neut" => f.gender = Some(Gender::Neut),
            _ => counters.skipped_unknown_field += 1,
        }
    }
    if let Some(s) = &rec.number {
        match s.as_str() {
            "Sg" => f.number = Some(Number::Sg),
            "Pl" => f.number = Some(Number::Pl),
            _ => counters.skipped_unknown_field += 1,
        }
    }
    if let Some(s) = &rec.case {
        match s.as_str() {
            "Nom" => f.case = Some(Case::Nom),
            "Gen" => f.case = Some(Case::Gen),
            "Dat" => f.case = Some(Case::Dat),
            "Acc" => f.case = Some(Case::Acc),
            _ => counters.skipped_unknown_field += 1,
        }
    }
    if let Some(s) = &rec.person {
        match s.as_str() {
            "1" => f.person = Some(Person::P1),
            "2" => f.person = Some(Person::P2),
            "3" => f.person = Some(Person::P3),
            _ => counters.skipped_unknown_field += 1,
        }
    }
    if let Some(s) = &rec.tense {
        match s.as_str() {
            "Pres" => f.tense = Some(Tense::Pres),
            "Past" => f.tense = Some(Tense::Past),
            _ => counters.skipped_unknown_field += 1,
        }
    }
    if let Some(s) = &rec.mood {
        match s.as_str() {
            "Ind" => f.mood = Some(Mood::Ind),
            "Sub1" => f.mood = Some(Mood::Sub1),
            "Sub2" => f.mood = Some(Mood::Sub2),
            "Imp" => f.mood = Some(Mood::Imp),
            _ => counters.skipped_unknown_field += 1,
        }
    }
    if let Some(s) = &rec.form {
        match s.as_str() {
            "Fin" => f.form = Some(VerbForm::Fin),
            "Inf" => f.form = Some(VerbForm::Inf),
            "InfZu" => f.form = Some(VerbForm::InfZu),
            "PtcPres" => f.form = Some(VerbForm::PtcPres),
            "PtcPerf" => f.form = Some(VerbForm::PtcPerf),
            _ => counters.skipped_unknown_field += 1,
        }
    }
    if let Some(s) = &rec.degree {
        match s.as_str() {
            "Pos" => f.degree = Some(Degree::Pos),
            "Cmp" => f.degree = Some(Degree::Cmp),
            "Sup" => f.degree = Some(Degree::Sup),
            _ => counters.skipped_unknown_field += 1,
        }
    }
    if let Some(s) = &rec.aux {
        match s.as_str() {
            "haben" => f.aux = Some(Aux::Haben),
            "sein" => f.aux = Some(Aux::Sein),
            "both" => f.aux = Some(Aux::Both),
            _ => counters.skipped_unknown_field += 1,
        }
    }
    if let Some(s) = &rec.pron_type {
        match s.as_str() {
            "Prs" => f.pron_type = Some(PronType::Prs),
            "Refl" => f.pron_type = Some(PronType::Refl),
            "Rel" => f.pron_type = Some(PronType::Rel),
            "Int" => f.pron_type = Some(PronType::Int),
            "Dem" => f.pron_type = Some(PronType::Dem),
            "Ind" => f.pron_type = Some(PronType::Ind),
            "Neg" => f.pron_type = Some(PronType::Neg),
            "Art" => f.pron_type = Some(PronType::Art),
            _ => counters.skipped_unknown_field += 1,
        }
    }
    f
}

fn parse_pos(s: &str) -> Option<UPOS> {
    Some(match s {
        "Noun" => UPOS::NOUN,
        "Verb" => UPOS::VERB,
        "Adj" => UPOS::ADJ,
        "Adv" => UPOS::ADV,
        "Pron" => UPOS::PRON,
        "Det" => UPOS::DET,
        "Num" => UPOS::NUM,
        "Adp" => UPOS::ADP,
        "Cconj" => UPOS::CCONJ,
        "Sconj" => UPOS::SCONJ,
        "Aux" => UPOS::AUX,
        "Part" => UPOS::PART,
        "Intj" => UPOS::INTJ,
        "Punct" => UPOS::PUNCT,
        "Sym" => UPOS::SYM,
        "X" => UPOS::X,
        "Propn" => UPOS::PROPN,
        _ => return None,
    })
}

fn parse_source(s: Option<&str>) -> Option<Source> {
    Some(match s? {
        "Lexicon" => Source::Lexicon,
        "Generated" => Source::Generated,
        "Guessed" => Source::Guessed,
        _ => return None,
    })
}

fn pos_label(p: UPOS) -> &'static str {
    match p {
        UPOS::NOUN => "Noun",
        UPOS::VERB => "Verb",
        UPOS::ADJ => "Adj",
        UPOS::ADV => "Adv",
        UPOS::PRON => "Pron",
        UPOS::DET => "Det",
        UPOS::NUM => "Num",
        UPOS::ADP => "Adp",
        UPOS::CCONJ => "Cconj",
        UPOS::SCONJ => "Sconj",
        UPOS::PROPN => "Propn",
        _ => "other",
    }
}

fn source_label(s: Source) -> &'static str {
    match s {
        Source::Lexicon => "Lexicon",
        Source::Generated => "Generated",
        Source::Guessed => "Guessed",
    }
}

struct Args {
    inputs: Vec<PathBuf>,
    fst_out: PathBuf,
    dat_out: PathBuf,
}

fn parse_args(argv: &[String]) -> Result<Args> {
    let mut inputs: Vec<PathBuf> = vec![
        PathBuf::from("data/wiktionary/processed/nouns.jsonl"),
        PathBuf::from("data/wiktionary/processed/verbs.jsonl"),
        PathBuf::from("data/wiktionary/processed/adjectives.jsonl"),
        PathBuf::from("data/wiktionary/processed/adverbs.jsonl"),
        PathBuf::from("data/wiktionary/processed/particles.jsonl"),
        PathBuf::from("data/wiktionary/processed/abbreviations.jsonl"),
        PathBuf::from("data/wiktionary/processed/propn.jsonl"),
    ];
    let mut fst_out = PathBuf::from("data/lexicon/lexicon.fst");
    let mut dat_out = PathBuf::from("data/lexicon/lexicon.dat");
    let mut explicit_inputs = false;

    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--input" => {
                i += 1;
                let p = argv.get(i).context("--input requires a value")?;
                if !explicit_inputs {
                    inputs.clear();
                    explicit_inputs = true;
                }
                inputs.push(PathBuf::from(p));
            }
            "--fst-out" => {
                i += 1;
                fst_out = PathBuf::from(argv.get(i).context("--fst-out requires a value")?);
            }
            "--dat-out" => {
                i += 1;
                dat_out = PathBuf::from(argv.get(i).context("--dat-out requires a value")?);
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
        inputs,
        fst_out,
        dat_out,
    })
}

fn print_usage() {
    eprintln!(
        "build-lexicon — build the runtime FST + side table from JSONL inputs\n\
\n\
Usage:\n\
  cargo run --release --features extractor --bin build-lexicon -- [options]\n\
\n\
Options:\n\
  --input <PATH>      Add an input JSONL file (default: nouns.jsonl + verbs.jsonl).\n\
                      Repeat to add multiple files. Specifying --input once\n\
                      replaces the defaults; further --input flags append.\n\
  --fst-out <PATH>    Output FST path  (default: data/lexicon/lexicon.fst)\n\
  --dat-out <PATH>    Output side-table path (default: data/lexicon/lexicon.dat)\n\
  --help              Show this message"
    );
}

fn humanize(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}
