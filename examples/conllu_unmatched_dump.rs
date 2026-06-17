//! Dump every (surface, gold_lemma, gold_pos) triple where our analyzer's
//! analyses do NOT include the gold lemma+pos pair, sorted by frequency.
//!
//! Usage:
//!   cargo run --release --example conllu_unmatched_dump -- <corpus_dir>...
//!
//! Output:
//!   `data/eval/unmatched.jsonl` — one record per (surface, gold_lemma,
//!   gold_pos) sorted by count desc. Schema:
//!     {"pos":"NOUN","surface":"Bröchten","gold_lemma":"Brötchen","count":42,
//!      "had_any_analysis":true,"reason":"lemma_disagrees"}
//!
//!   `reason` is one of:
//!     - "not_in_lexicon":   our analyzer returned nothing for the surface
//!     - "pos_disagrees":    we returned an analysis but no analysis had the
//!                           gold POS
//!     - "lemma_disagrees":  POS matched somewhere but no analysis paired the
//!                           gold POS with the gold lemma
//!
//! Why three reasons: each points to a different fix.
//!   - "not_in_lexicon"     → extractor coverage gap or build-time filter
//!   - "pos_disagrees"      → tagger convention difference (e.g. UDPipe's DET
//!                            vs our NOUN/PRON)
//!   - "lemma_disagrees"    → lemmatization convention or paradigm gap

use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use de_morph::Analyzer;

const FST: &str = "data/lexicon/lexicon.fst";
const DAT: &str = "data/lexicon/lexicon.dat";
const OUTPUT: &str = "data/eval/unmatched.jsonl";

#[derive(Default)]
struct MissRecord {
    count: u64,
    had_any_analysis: bool,
    pos_matched_somewhere: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let argv: Vec<String> = std::env::args().collect();
    if argv.len() < 2 {
        eprintln!("usage: conllu_unmatched_dump <path>...  (path = .conllu file OR directory)");
        std::process::exit(2);
    }
    eprintln!("Loading lexicon...");
    let analyzer = Analyzer::open(FST, DAT)?.with_swiss_orthography(true);
    eprintln!("  loaded in 0.01s");

    let mut misses: HashMap<(String, String, String), MissRecord> = HashMap::new();
    let t = Instant::now();
    let mut tok_total: u64 = 0;
    let mut tok_miss: u64 = 0;

    for arg in argv.iter().skip(1) {
        let p = PathBuf::from(arg);
        if p.is_dir() {
            for entry in fs::read_dir(&p)? {
                let entry = entry?;
                let pp = entry.path();
                if pp.extension().and_then(|s| s.to_str()) == Some("conllu") {
                    process_file(&pp, &analyzer, &mut misses, &mut tok_total, &mut tok_miss)?;
                }
            }
        } else if p.extension().and_then(|s| s.to_str()) == Some("conllu") {
            process_file(&p, &analyzer, &mut misses, &mut tok_total, &mut tok_miss)?;
        }
    }

    eprintln!(
        "Scanned {tok_total} tokens in {:.1}s; {tok_miss} unmatched ({:.1}%)",
        t.elapsed().as_secs_f64(),
        100.0 * tok_miss as f64 / tok_total.max(1) as f64
    );

    // Sort by count desc; tie-break by surface to keep output deterministic.
    let mut sorted: Vec<((String, String, String), MissRecord)> = misses.into_iter().collect();
    sorted.sort_by(|a, b| b.1.count.cmp(&a.1.count).then_with(|| a.0.0.cmp(&b.0.0)));

    if let Some(parent) = Path::new(OUTPUT).parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(OUTPUT)?);
    for ((pos, surface, gold_lemma), m) in &sorted {
        let reason = if !m.had_any_analysis {
            "not_in_lexicon"
        } else if !m.pos_matched_somewhere {
            "pos_disagrees"
        } else {
            "lemma_disagrees"
        };
        writeln!(
            writer,
            "{{\"pos\":\"{}\",\"surface\":{},\"gold_lemma\":{},\"count\":{},\"had_any_analysis\":{},\"reason\":\"{}\"}}",
            pos,
            json_string(surface),
            json_string(gold_lemma),
            m.count,
            m.had_any_analysis,
            reason
        )?;
    }
    eprintln!("Wrote {} unique miss records to {OUTPUT}", sorted.len());

    // Summary: distinct miss counts per POS, with breakdown by reason.
    let mut by_pos: HashMap<&str, (u64, u64, u64, u64)> = HashMap::new();
    for ((pos, _, _), m) in &sorted {
        let entry = by_pos.entry(pos.as_str()).or_default();
        entry.0 += 1;
        entry.1 += m.count;
        if !m.had_any_analysis {
            entry.2 += m.count;
        } else if !m.pos_matched_somewhere {
            entry.3 += m.count;
        }
    }
    let mut by_pos_v: Vec<_> = by_pos.into_iter().collect();
    by_pos_v.sort_by(|a, b| b.1.1.cmp(&a.1.1));
    println!("\nMiss breakdown by gold POS (top reasons):");
    println!(
        "  {:<8} {:>10} {:>10}  {:>10} {:>10}",
        "POS", "uniq", "tokens", "not_in_lex", "pos_disagr"
    );
    for (pos, (uniq, total, nil, pd)) in &by_pos_v {
        println!(
            "  {:<8} {:>10} {:>10}  {:>10} {:>10}",
            pos, uniq, total, nil, pd
        );
    }

    Ok(())
}

fn process_file(
    path: &Path,
    analyzer: &Analyzer,
    misses: &mut HashMap<(String, String, String), MissRecord>,
    tok_total: &mut u64,
    tok_miss: &mut u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line?;
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 5 || cols[0].contains('-') {
            continue;
        }
        let surface = cols[1];
        let gold_lemma = cols[2];
        let gold_upos = cols[3];

        // Skip non-alphabetic tokens (punctuation, digits).
        if !surface.chars().next().is_some_and(|c| c.is_alphabetic()) {
            continue;
        }
        *tok_total += 1;

        let analyses = analyzer.analyze(surface);
        let gold_pos = upos_to_pos(gold_upos);
        // PROPN-tolerant match: gold PROPN accepts our Noun too, since
        // the bulk of proper nouns get tagged Noun by the Substantiv
        // extractor (only the curated abbreviation table emits Propn).
        let pos_matches = |our: de_morph::UPOS, gold: de_morph::UPOS| -> bool {
            our == gold || (gold == de_morph::UPOS::PROPN && our == de_morph::UPOS::NOUN)
        };
        let any_hit = !analyses.is_empty();
        let pos_match_somewhere = gold_pos
            .map(|gp| analyses.iter().any(|a| pos_matches(a.pos, gp)))
            .unwrap_or(false);
        let joint_match = gold_pos
            .map(|gp| analyses.iter().any(|a| pos_matches(a.pos, gp) && a.lemma == gold_lemma))
            .unwrap_or(false);

        if joint_match {
            continue;
        }
        *tok_miss += 1;
        let key = (
            gold_upos.to_string(),
            surface.to_string(),
            gold_lemma.to_string(),
        );
        let rec = misses.entry(key).or_default();
        rec.count += 1;
        rec.had_any_analysis = rec.had_any_analysis || any_hit;
        rec.pos_matched_somewhere = rec.pos_matched_somewhere || pos_match_somewhere;
    }
    Ok(())
}

fn upos_to_pos(upos: &str) -> Option<de_morph::UPOS> {
    use de_morph::UPOS;
    Some(match upos {
        "NOUN" => UPOS::NOUN,
        "PROPN" => UPOS::PROPN,
        "VERB" | "AUX" => UPOS::VERB,
        "ADJ" => UPOS::ADJ,
        "ADV" => UPOS::ADV,
        "PRON" => UPOS::PRON,
        "DET" => UPOS::DET,
        "NUM" => UPOS::NUM,
        "ADP" => UPOS::ADP,
        "CCONJ" => UPOS::CCONJ,
        "SCONJ" => UPOS::SCONJ,
        "PART" => UPOS::PART,
        "PUNCT" => UPOS::PUNCT,
        "INTJ" => UPOS::INTJ,
        _ => return None,
    })
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
