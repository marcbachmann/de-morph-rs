//! Validate the heuristic compound splitter against Wiktionary's
//! curated decomposition.
//!
//! For each entry in compounds.jsonl, we:
//!   1. Take the lemma (e.g. "Wörterbuch")
//!   2. Take Wiktionary's parts (e.g. ["Wort", "Buch"])
//!   3. Run `lex.split_compound_detailed_ranked(lemma)` and inspect
//!      the top-1 + top-k candidates
//!   4. Report match rates: top-1 exact, top-1 part-set, top-5 any-match
//!
//! We also track WHICH compounds the splitter completely misses (no
//! split returned) — those are the most informative for understanding
//! where rule limits hit.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::time::Instant;

use de_morph::Lexicon;

const FST: &str = "data/lexicon/lexicon.fst";
const DAT: &str = "data/lexicon/lexicon.dat";
const COMPOUNDS: &str = "data/wiktionary/processed/compounds.jsonl";

#[derive(Default, Debug)]
struct Tally {
    total: u64,
    splitter_no_split: u64,
    top1_parts_set_matches: u64,
    top1_parts_order_matches: u64,
    top5_parts_set_matches: u64,
    fugenelement_match: u64,
    fugenelement_total: u64, // denominator only counts compounds with a Wiktionary-recorded Fugenelement
}

#[derive(Debug)]
struct CompoundRecord {
    lemma: String,
    parts: Vec<String>,
    fugenelement: Option<String>,
    compound_type: String,
}

fn parse_jsonl_line(line: &str) -> Option<CompoundRecord> {
    // Minimal JSON parsing — avoids pulling in serde just for the
    // diagnostic.
    let lemma = pull_string_field(line, "\"lemma\":\"")?;
    let parts = pull_string_array(line, "\"parts\":[")?;
    let fugenelement = pull_optional_string(line, "\"fugenelement\":");
    let compound_type = pull_string_field(line, "\"compound_type\":\"")
        .unwrap_or_else(|| "Determinativ".to_string());
    Some(CompoundRecord {
        lemma,
        parts,
        fugenelement,
        compound_type,
    })
}

fn pull_string_field(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn pull_string_array(line: &str, key: &str) -> Option<Vec<String>> {
    let start = line.find(key)? + key.len();
    let rest = &line[start..];
    let end = rest.find(']')?;
    let array_body = &rest[..end];
    let mut out = Vec::new();
    let mut chars = array_body.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            let mut s = String::new();
            for ch in chars.by_ref() {
                if ch == '"' {
                    break;
                }
                s.push(ch);
            }
            out.push(s);
        }
    }
    Some(out)
}

fn pull_optional_string(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
    let rest = &line[start..];
    if rest.starts_with("null") {
        return None;
    }
    if !rest.starts_with('"') {
        return None;
    }
    let after_quote = &rest[1..];
    let end = after_quote.find('"')?;
    Some(after_quote[..end].to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let lex = Lexicon::open(FST, DAT)?;
    eprintln!("Lexicon: {} lemmas, {} surfaces",
              lex.num_lemmas(), lex.num_surfaces());
    eprintln!("Reading {COMPOUNDS}");
    let file = File::open(COMPOUNDS)?;
    let reader = BufReader::new(file);

    let t = Instant::now();
    let mut tally = Tally::default();
    let mut sample_no_split: Vec<String> = Vec::new();
    let mut sample_disagreement: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let Some(rec) = parse_jsonl_line(&line) else { continue };
        // Only evaluate against Determinativ compounds; the others are
        // too few and have idiosyncratic structures (Possessivkompositum
        // attaches a suffix like -chen rather than another noun).
        if rec.compound_type != "Determinativ" {
            continue;
        }
        // Need ≥ 2 parts for a sensible comparison.
        if rec.parts.len() < 2 {
            continue;
        }
        tally.total += 1;

        let splits = lex.split_compound_detailed_ranked(&rec.lemma);
        if splits.is_empty() {
            tally.splitter_no_split += 1;
            if sample_no_split.len() < 20 {
                sample_no_split.push(format!(
                    "{} ({})",
                    rec.lemma,
                    rec.parts.join("+")
                ));
            }
            continue;
        }
        let wikt_set: HashSet<String> = rec.parts.iter().cloned().collect();

        // Top-1 matching.
        let top = &splits[0].0;
        let top_set: HashSet<String> = top.parts.iter().cloned().collect();
        let order_eq = top.parts == rec.parts;
        let set_eq = top_set == wikt_set;
        if order_eq {
            tally.top1_parts_order_matches += 1;
        }
        if set_eq {
            tally.top1_parts_set_matches += 1;
        }
        if !order_eq && sample_disagreement.len() < 20 {
            sample_disagreement.push((
                rec.lemma.clone(),
                rec.parts.clone(),
                top.parts.clone(),
            ));
        }

        // Top-5 set match.
        let any_top5_set = splits.iter().take(5).any(|(s, _)| {
            let set: HashSet<String> = s.parts.iter().cloned().collect();
            set == wikt_set
        });
        if any_top5_set {
            tally.top5_parts_set_matches += 1;
        }

        // Fugenelement comparison (only count when Wiktionary specified one).
        if let Some(wikt_fuge) = &rec.fugenelement {
            tally.fugenelement_total += 1;
            // Find the first split with parts matching Wiktionary, then
            // compare its linker.
            let matching = splits.iter().find(|(s, _)| {
                let set: HashSet<String> = s.parts.iter().cloned().collect();
                set == wikt_set
            });
            if let Some((s, _)) = matching {
                // Wiktionary fugenelement strings have leading "-" stripped
                // already (`er` not `-er`). Compare to first linker.
                let our_fuge = s.linkers.first().cloned().unwrap_or_default();
                if our_fuge == *wikt_fuge {
                    tally.fugenelement_match += 1;
                }
            }
        }
    }

    let elapsed = t.elapsed().as_secs_f64();
    eprintln!("Compared {} compounds in {:.1}s", tally.total, elapsed);

    let pct = |n: u64, d: u64| {
        if d == 0 { 0.0 } else { 100.0 * n as f64 / d as f64 }
    };

    println!("\n=== Splitter vs. Wiktionary ground truth ===");
    println!("  Compounds evaluated:               {}", tally.total);
    println!("  No split returned (splitter miss): {} ({:.1}%)",
             tally.splitter_no_split,
             pct(tally.splitter_no_split, tally.total));

    let with_split = tally.total - tally.splitter_no_split;
    println!("\n  Of the {} compounds the splitter handled:", with_split);
    println!("  Top-1 parts (exact order):         {} ({:.1}%)",
             tally.top1_parts_order_matches,
             pct(tally.top1_parts_order_matches, with_split));
    println!("  Top-1 parts (any order):           {} ({:.1}%)",
             tally.top1_parts_set_matches,
             pct(tally.top1_parts_set_matches, with_split));
    println!("  Top-5 parts (any-of-top-5 match):  {} ({:.1}%)",
             tally.top5_parts_set_matches,
             pct(tally.top5_parts_set_matches, with_split));

    println!("\n  Fugenelement agreement (only counted when Wiktionary explicitly annotates one):");
    println!("    Linker match: {}/{} ({:.1}%)",
             tally.fugenelement_match,
             tally.fugenelement_total,
             pct(tally.fugenelement_match, tally.fugenelement_total));

    println!("\nExamples — splitter returned no split:");
    for s in &sample_no_split[..sample_no_split.len().min(10)] {
        println!("  {s}");
    }

    println!("\nExamples — top-1 disagreement (Wiktionary truth vs. our top pick):");
    for (lemma, wikt, ours) in &sample_disagreement[..sample_disagreement.len().min(10)] {
        println!("  {lemma}:  Wikt={:?} vs ours={:?}", wikt, ours);
    }

    Ok(())
}
