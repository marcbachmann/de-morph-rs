//! Evaluate the analyzer against CoNLL-U gold data.
//!
//! For each token in the input corpus, look up the surface form and
//! check whether any returned analysis matches the gold lemma + POS.
//! Reports coverage, lemma recall, POS accuracy, and joint
//! (lemma + POS) accuracy, broken down by POS.
//!
//! Run with:
//!   cargo run --release --example conllu_eval -- <conllu-dir> [N]
//! where N (optional) limits the number of files.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Instant;

use de_morph::analysis::UPOS;
use de_morph::{Analyzer, Lexicon};

const FST_PATH: &str = "data/lexicon/lexicon.fst";
const DAT_PATH: &str = "data/lexicon/lexicon.dat";

#[derive(Default, Debug, Clone, Copy)]
struct Counts {
    total: u64,
    in_lex: u64,        // analyzer returned at least one (non-Guessed) analysis
    any_hit: u64,       // analyzer returned at least one analysis (lex or guess)
    lemma_match: u64,   // any returned analysis has lemma == gold
    pos_match: u64,     // any returned analysis has pos == gold
    joint_match: u64,   // any returned analysis matches both lemma and pos
    top1_lemma_match: u64, // first returned analysis matches gold lemma
    top1_pos_match: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let argv: Vec<String> = std::env::args().collect();
    if argv.len() < 2 {
        return Err("usage: conllu_eval <path>...  (path = .conllu file OR directory)".into());
    }

    eprintln!("Loading lexicon...");
    let t0 = Instant::now();
    let lex = Lexicon::open(FST_PATH, DAT_PATH)?;
    // Enable Swiss-orthography for eval — out-udpipe is heavy on
    // Swiss/Austrian text, and the UD treebanks mix both. The flag is
    // off in the library default; eval is the right place to opt in
    // so we measure the bridge's contribution rather than excluding it.
    let analyzer = Analyzer::from_lexicon(lex).with_swiss_orthography(true);
    eprintln!(
        "  loaded in {:.2}s",
        t0.elapsed().as_secs_f64()
    );

    // Expand any input path: a .conllu file is taken as-is, a
    // directory is searched (one level deep) for .conllu files. Each
    // input path becomes its own labelled corpus in the per-corpus
    // breakdown.
    let mut corpora: Vec<(String, Vec<PathBuf>)> = Vec::new();
    for arg in &argv[1..] {
        let path = Path::new(arg);
        if !path.exists() {
            return Err(format!("path does not exist: {arg}").into());
        }
        let label = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(arg)
            .to_string();
        let files = if path.is_dir() {
            let mut fs: Vec<PathBuf> = std::fs::read_dir(path)?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "conllu"))
                .collect();
            fs.sort();
            fs
        } else {
            vec![path.to_path_buf()]
        };
        if files.is_empty() {
            eprintln!("(no .conllu files under {arg})");
            continue;
        }
        corpora.push((label, files));
    }

    let mut overall = Counts::default();
    let mut by_pos: HashMap<&'static str, Counts> = HashMap::new();
    let mut unmatched_examples: HashMap<&'static str, Vec<(String, String)>> = HashMap::new();
    let t_eval = Instant::now();

    // Per-corpus counts so we can report per-treebank in addition to
    // the cross-corpus aggregate.
    let mut per_corpus: Vec<(String, Counts)> = Vec::new();

    for (label, files) in &corpora {
        eprintln!("--- {label} ({} files) ---", files.len());
        let mut corpus_counts = Counts::default();
        for path in files {
            eprintln!("  {}", path.file_name().unwrap().to_string_lossy());
            let mut file_counts = Counts::default();
            process_file(
                path,
                &analyzer,
                &mut file_counts,
                &mut by_pos,
                &mut unmatched_examples,
            )?;
            merge(&mut corpus_counts, &file_counts);
            merge(&mut overall, &file_counts);
        }
        per_corpus.push((label.clone(), corpus_counts));
    }

    let elapsed = t_eval.elapsed().as_secs_f64();
    eprintln!("\n=== Aggregate ({} tokens, {:.1}s, {:.0}k tok/s) ===",
             overall.total, elapsed,
             overall.total as f64 / elapsed / 1000.0);

    print_counts("OVERALL", &overall);

    println!("\nBy corpus:");
    println!("  {:<24} {:>10} {:>10} {:>10} {:>10} {:>10}",
             "corpus", "tokens", "cov", "lemma%", "pos%", "joint%");
    for (label, c) in &per_corpus {
        if c.total == 0 { continue; }
        println!("  {:<24} {:>10} {:>9.1}% {:>9.1}% {:>9.1}% {:>9.1}%",
                 label,
                 c.total,
                 100.0 * c.any_hit as f64 / c.total as f64,
                 100.0 * c.lemma_match as f64 / c.total as f64,
                 100.0 * c.pos_match as f64 / c.total as f64,
                 100.0 * c.joint_match as f64 / c.total as f64);
    }

    let pos_order: &[&str] = &[
        "NOUN", "VERB", "ADJ", "ADV", "PRON", "DET", "NUM", "ADP",
        "CCONJ", "SCONJ", "AUX", "PART", "PROPN", "PUNCT", "X",
    ];
    println!("\nBy gold POS:");
    println!("  {:<7} {:>10} {:>10} {:>10} {:>10} {:>10}",
             "POS", "tokens", "coverage", "lemma%", "pos%", "joint%");
    for pos in pos_order {
        if let Some(c) = by_pos.get(pos) {
            if c.total == 0 { continue; }
            println!("  {:<7} {:>10} {:>9.1}% {:>9.1}% {:>9.1}% {:>9.1}%",
                     pos,
                     c.total,
                     100.0 * c.any_hit as f64 / c.total as f64,
                     100.0 * c.lemma_match as f64 / c.total as f64,
                     100.0 * c.pos_match as f64 / c.total as f64,
                     100.0 * c.joint_match as f64 / c.total as f64);
        }
    }

    // Show a few unmatched examples per POS to help understand misses.
    println!("\nA few unmatched examples (surface → gold lemma):");
    for pos in pos_order {
        if let Some(samples) = unmatched_examples.get(pos) {
            if samples.is_empty() { continue; }
            print!("  {}: ", pos);
            for (s, l) in samples.iter().take(6) {
                print!("{s}→{l}  ");
            }
            println!();
        }
    }

    Ok(())
}

fn process_file(
    path: &Path,
    analyzer: &Analyzer,
    overall: &mut Counts,
    by_pos: &mut HashMap<&'static str, Counts>,
    unmatched: &mut HashMap<&'static str, Vec<(String, String)>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line?;
        // Skip comments, blank lines, and multi-word tokens.
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 5 {
            continue;
        }
        // Skip multi-word ranges (id like "1-2") — those are surface
        // contractions; the individual tokens follow as separate lines.
        if cols[0].contains('-') {
            continue;
        }
        let surface = cols[1];
        let gold_lemma = cols[2];
        let gold_upos = cols[3];

        let analyses = analyzer.analyze(surface);
        let gold_pos = upos_to_pos(gold_upos);

        // PROPN-tolerant match: gold PROPN accepts both our Propn AND
        // Noun (the noun extractor tags most proper nouns as Noun
        // because Wiktionary's Substantiv template doesn't separate
        // PROPN; only the curated abbreviation table emits Propn).
        let pos_matches = |our: UPOS, gold: UPOS| -> bool {
            our == gold || (gold == UPOS::PROPN && our == UPOS::NOUN)
        };

        let any_hit = !analyses.is_empty();
        let in_lex = analyses
            .iter()
            .any(|a| a.source == de_morph::Source::Lexicon || a.source == de_morph::Source::Generated);
        let lemma_match = analyses.iter().any(|a| a.lemma == gold_lemma);
        let pos_match = gold_pos
            .map(|gp| analyses.iter().any(|a| pos_matches(a.pos, gp)))
            .unwrap_or(false);
        let joint_match = gold_pos
            .map(|gp| analyses.iter().any(|a| pos_matches(a.pos, gp) && a.lemma == gold_lemma))
            .unwrap_or(false);
        let top1_lemma_match = analyses.first().is_some_and(|a| a.lemma == gold_lemma);
        let top1_pos_match = gold_pos
            .map(|gp| analyses.first().is_some_and(|a| pos_matches(a.pos, gp)))
            .unwrap_or(false);

        let update = |c: &mut Counts| {
            c.total += 1;
            c.any_hit += any_hit as u64;
            c.in_lex += in_lex as u64;
            c.lemma_match += lemma_match as u64;
            c.pos_match += pos_match as u64;
            c.joint_match += joint_match as u64;
            c.top1_lemma_match += top1_lemma_match as u64;
            c.top1_pos_match += top1_pos_match as u64;
        };
        update(overall);
        let upos_key = static_upos_label(gold_upos);
        update(by_pos.entry(upos_key).or_default());

        if !joint_match && !surface.chars().next().is_some_and(|c| !c.is_alphabetic()) {
            // Collect up to 50 unmatched examples per POS for debugging.
            let bucket = unmatched.entry(upos_key).or_default();
            if bucket.len() < 50 {
                bucket.push((surface.to_string(), gold_lemma.to_string()));
            }
        }
    }
    Ok(())
}

fn merge(dst: &mut Counts, src: &Counts) {
    dst.total += src.total;
    dst.in_lex += src.in_lex;
    dst.any_hit += src.any_hit;
    dst.lemma_match += src.lemma_match;
    dst.pos_match += src.pos_match;
    dst.joint_match += src.joint_match;
    dst.top1_lemma_match += src.top1_lemma_match;
    dst.top1_pos_match += src.top1_pos_match;
}

fn print_counts(label: &str, c: &Counts) {
    if c.total == 0 {
        println!("{label}: (no tokens)");
        return;
    }
    let pct = |n: u64| 100.0 * n as f64 / c.total as f64;
    println!("  {label}:");
    println!("    tokens                   {:>10}", c.total);
    println!("    any analysis returned    {:>9.1}%   ({})", pct(c.any_hit), c.any_hit);
    println!("    in lexicon (not guessed) {:>9.1}%   ({})", pct(c.in_lex), c.in_lex);
    println!("    lemma in any analysis    {:>9.1}%   ({})", pct(c.lemma_match), c.lemma_match);
    println!("    pos in any analysis      {:>9.1}%   ({})", pct(c.pos_match), c.pos_match);
    println!("    joint (lemma + pos) any  {:>9.1}%   ({})", pct(c.joint_match), c.joint_match);
    println!("    lemma first analysis     {:>9.1}%", pct(c.top1_lemma_match));
    println!("    pos   first analysis     {:>9.1}%", pct(c.top1_pos_match));
}

/// Map CoNLL-U UPOS string → our UPOS enum. Returns None for tags we
/// PROPN now maps to its own UPOS::PROPN (since v3 format added the
/// 17th UPOS variant), so we no longer lump it into Noun. AUX still
/// maps to Verb because the lexicon doesn't distinguish auxiliaries
/// at the POS level — auxiliary-ness is a feature of the lemma set,
/// not a separate POS in our enum.
fn upos_to_pos(upos: &str) -> Option<UPOS> {
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
        "INTJ" => UPOS::INTJ,
        "PUNCT" => UPOS::PUNCT,
        "SYM" => UPOS::SYM,
        "X" => UPOS::X,
        _ => return None,
    })
}

fn static_upos_label(upos: &str) -> &'static str {
    match upos {
        "NOUN" => "NOUN",
        "VERB" => "VERB",
        "ADJ" => "ADJ",
        "ADV" => "ADV",
        "PRON" => "PRON",
        "DET" => "DET",
        "NUM" => "NUM",
        "ADP" => "ADP",
        "CCONJ" => "CCONJ",
        "SCONJ" => "SCONJ",
        "AUX" => "AUX",
        "PART" => "PART",
        "PROPN" => "PROPN",
        "INTJ" => "INTJ",
        "PUNCT" => "PUNCT",
        "SYM" => "SYM",
        "X" => "X",
        _ => "OTHER",
    }
}
