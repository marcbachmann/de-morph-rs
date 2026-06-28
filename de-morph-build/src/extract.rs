//! `de-morph-build extract <kind>` — extract analyses from a Wiktionary dump.
//!
//! Unifies the former `extract-*` binaries. Each `kind` reads the dump,
//! runs the matching extractor over every main-namespace page, and writes
//! JSON-lines to `data/wiktionary/processed/<kind>.jsonl` (override with
//! `--output`). The output schema is the union of all per-POS fields, each
//! omitted when unset — byte-compatible with what `build-lexicon` ingests
//! and what the splitter eval reads.
//!
//! Usage:
//!   de-morph-build extract <kind> [--input PATH] [--output PATH] [--limit N]
//!
//! Kinds: nouns verbs adjectives adverbs particles abbreviations propn
//!        pronouns compounds
//!
//! Attribution: JSONL carries `source_title` for CC BY-SA 4.0 compliance
//! (<https://creativecommons.org/licenses/by-sa/4.0/legalcode> § 3.a).

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use de_morph::analysis::{
    Aux, Case, Declension, Degree, Gender, Mood, Number, Person, PronType, Source, Tense, UPOS,
    VerbForm,
};
use de_morph::paradigm::generate_closed_class_entries;
use crate::wiktionary::abbreviation::extract_abbreviations;
use crate::wiktionary::adjective::extract_adjectives;
use crate::wiktionary::adverb::{extract_adverbs, extract_particles};
use crate::wiktionary::compound::extract_compounds;
use crate::wiktionary::dump::{Page, PageReader};
use crate::wiktionary::noun::extract_nouns;
use crate::wiktionary::pronoun::extract_pronouns;
use crate::wiktionary::propn::extract_proper_nouns;
use crate::wiktionary::verb::extract_verbs;
use crate::wiktionary::ExtractedEntry;

const DEFAULT_INPUT: &str = "data/wiktionary/raw/dewiktionary-20260601-pages-articles.xml.bz2";
const PROGRESS_EVERY: u64 = 50_000;

/// The full union of fields any per-POS extractor emits. Every optional
/// field is skipped when `None`, so each record reproduces exactly the
/// shape its dedicated extractor produced (a noun never sets `tense`, a
/// verb never sets `gender`, …). `build-lexicon` reads this by field name.
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
    person: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tense: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mood: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    form: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    degree: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    declension: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pron_type: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aux: Option<&'static str>,
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
        person: e.features.person.map(person_str),
        tense: e.features.tense.map(tense_str),
        mood: e.features.mood.map(mood_str),
        form: e.features.form.map(form_str),
        degree: e.features.degree.map(degree_str),
        declension: e.features.declension.map(declension_str),
        pron_type: e.features.pron_type.map(pron_type_str),
        aux: e.features.aux.map(aux_str),
        source: source_str(e.source),
        source_title: &e.source_title,
    }
}

pub fn run(args: &[String]) -> Result<()> {
    let mut kind: Option<String> = None;
    let mut input = PathBuf::from(DEFAULT_INPUT);
    let mut output: Option<PathBuf> = None;
    let mut limit: Option<u64> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--input" => {
                i += 1;
                input = args.get(i).context("--input requires a value")?.into();
            }
            "--output" => {
                i += 1;
                output = Some(args.get(i).context("--output requires a value")?.into());
            }
            "--limit" => {
                i += 1;
                limit = Some(
                    args.get(i)
                        .context("--limit requires a value")?
                        .parse::<u64>()
                        .context("--limit must be an integer")?,
                );
            }
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            other if other.starts_with('-') => {
                print_usage();
                bail!("unknown argument: {other}");
            }
            other if kind.is_none() => kind = Some(other.to_string()),
            other => {
                print_usage();
                bail!("unexpected extra argument: {other}");
            }
        }
        i += 1;
    }

    let kind = kind.context("missing <kind> (try `de-morph-build extract --help`)")?;
    let output = output
        .unwrap_or_else(|| PathBuf::from(format!("data/wiktionary/processed/{kind}.jsonl")));

    eprintln!("Reading {}", input.display());
    eprintln!("Writing {}", output.display());
    if let Some(n) = limit {
        eprintln!("Limit:   {n} main-namespace pages");
    }

    // Per-POS extractors with the uniform (title, text) -> entries shape.
    type EntryFn = fn(&str, &str) -> Vec<ExtractedEntry>;
    let entry_fn: Option<EntryFn> = match kind.as_str() {
        "nouns" => Some(extract_nouns),
        "verbs" => Some(extract_verbs),
        "adjectives" => Some(extract_adjectives),
        "adverbs" => Some(extract_adverbs),
        "particles" => Some(extract_particles),
        "abbreviations" => Some(extract_abbreviations),
        "propn" => Some(extract_proper_nouns),
        _ => None,
    };

    if let Some(f) = entry_fn {
        drive(&input, &output, limit, |page, w| {
            let mut n = 0;
            for e in f(&page.title, &page.text) {
                write_entry(w, &e)?;
                n += 1;
            }
            Ok(n)
        })
    } else if kind == "pronouns" {
        // Pronouns are deduplicated against the hand-curated closed-class
        // lemmas baked in at build time.
        let covered = covered_lemmas();
        eprintln!("Excluding {} hand-curated closed-class lemmas", covered.len());
        drive(&input, &output, limit, |page, w| {
            let mut n = 0;
            for e in extract_pronouns(&page.title, &page.text, &covered) {
                write_entry(w, &e)?;
                n += 1;
            }
            Ok(n)
        })
    } else if kind == "compounds" {
        // Compounds emit a different record (parts + Fugenelement) that
        // feeds the runtime splitter, not the FST.
        drive(&input, &output, limit, |page, w| {
            let mut n = 0;
            for e in extract_compounds(&page.title, &page.text) {
                serde_json::to_writer(&mut *w, &e).context("write json")?;
                w.write_all(b"\n").context("newline")?;
                n += 1;
            }
            Ok(n)
        })
    } else {
        print_usage();
        bail!("unknown kind: {kind}");
    }
}

/// Open the dump, stream main-namespace pages through `per_page`, and
/// write the JSONL output with periodic progress + a final summary.
fn drive<F>(input: &Path, output: &Path, limit: Option<u64>, mut per_page: F) -> Result<()>
where
    F: FnMut(&Page, &mut BufWriter<File>) -> Result<u64>,
{
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).context("creating output directory")?;
    }
    let out_file = File::create(output).context("opening output file")?;
    let mut writer = BufWriter::with_capacity(1 << 20, out_file);

    let reader = PageReader::open_bz2(input)
        .with_context(|| format!("opening dump at {}", input.display()))?;

    let start = Instant::now();
    let mut pages_total: u64 = 0;
    let mut pages_main: u64 = 0;
    let mut entries: u64 = 0;
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

        entries += per_page(&page, &mut writer)?;

        if pages_main % PROGRESS_EVERY == 0 {
            eprintln!(
                "  pages_total={pages_total} pages_main={pages_main} entries={entries} elapsed={:.1}s",
                start.elapsed().as_secs_f64()
            );
        }
        if let Some(limit) = limit {
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
    eprintln!("  errors      = {errors}");
    eprintln!("  elapsed     = {elapsed:.1}s");
    eprintln!(
        "  rate        = {:.0} pages/s",
        pages_total as f64 / elapsed.max(0.001)
    );
    Ok(())
}

fn write_entry(w: &mut BufWriter<File>, e: &ExtractedEntry) -> Result<()> {
    serde_json::to_writer(&mut *w, &entry_to_record(e)).context("write json")?;
    w.write_all(b"\n").context("newline")?;
    Ok(())
}

fn covered_lemmas() -> HashSet<String> {
    generate_closed_class_entries()
        .into_iter()
        .map(|(_, a)| a.lemma.into_owned())
        .collect()
}

fn print_usage() {
    eprintln!(
        "de-morph-build extract — extract analyses from a Wiktionary dump\n\
\n\
Usage:\n\
  de-morph-build extract <kind> [options]\n\
\n\
Kinds:\n\
  nouns verbs adjectives adverbs particles abbreviations propn pronouns compounds\n\
\n\
Options:\n\
  --input <PATH>    Input .xml.bz2 dump  (default: {DEFAULT_INPUT})\n\
  --output <PATH>   Output .jsonl        (default: data/wiktionary/processed/<kind>.jsonl)\n\
  --limit <N>       Stop after N main-namespace pages\n\
  --help            Show this message"
    );
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
        UPOS::AUX => "Aux",
        UPOS::PART => "Part",
        UPOS::INTJ => "Intj",
        UPOS::PUNCT => "Punct",
        UPOS::SYM => "Sym",
        UPOS::X => "X",
        UPOS::PROPN => "Propn",
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

fn person_str(p: Person) -> &'static str {
    match p {
        Person::P1 => "1",
        Person::P2 => "2",
        Person::P3 => "3",
    }
}

fn tense_str(t: Tense) -> &'static str {
    match t {
        Tense::Pres => "Pres",
        Tense::Past => "Past",
    }
}

fn mood_str(m: Mood) -> &'static str {
    match m {
        Mood::Ind => "Ind",
        Mood::Sub1 => "Sub1",
        Mood::Sub2 => "Sub2",
        Mood::Imp => "Imp",
    }
}

fn form_str(f: VerbForm) -> &'static str {
    match f {
        VerbForm::Fin => "Fin",
        VerbForm::Inf => "Inf",
        VerbForm::InfZu => "InfZu",
        VerbForm::PtcPres => "PtcPres",
        VerbForm::PtcPerf => "PtcPerf",
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

fn pron_type_str(pt: PronType) -> &'static str {
    match pt {
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

fn aux_str(a: Aux) -> &'static str {
    match a {
        Aux::Haben => "haben",
        Aux::Sein => "sein",
        Aux::Both => "both",
    }
}
