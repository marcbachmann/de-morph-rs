//! Throughput + memory benchmark for the runtime analyzer.
//!
//! Loads the embedded lexicon (zero-copy `from_static`), collects every
//! surface form, then times `Lexicon::analyze` over all of them across
//! several passes. Run under `/usr/bin/time -l` to capture max RSS:
//!
//!   cargo build --release --example bench_analyze
//!   /usr/bin/time -l ./target/release/examples/bench_analyze 5
//!
//! Modes (arg 1):
//!   sweep [passes]   throughput over every surface (default, passes=5)
//!   load             from_static load only — dat stays demand-paged
//!   loadbytes        from_bytes load only — dat copied into the heap
//! Combine with `/usr/bin/time -l` to read max RSS per mode.

use std::time::Instant;

use de_morph::Lexicon;
use fst::{Map as FstMap, Streamer};

static LEXICON_FST: &[u8] = include_bytes!("../data/lexicon/lexicon.fst");
static LEXICON_DAT: &[u8] = include_bytes!("../data/lexicon/lexicon.dat");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "sweep".into());

    // Load-only modes isolate the lexicon's resident footprint (no word
    // list, no analysis churn) — `from_static` leaves the side table
    // demand-paged in the binary image, `from_bytes` copies it to the heap.
    if mode == "load" || mode == "loadbytes" {
        let t = Instant::now();
        let lex = if mode == "loadbytes" {
            Lexicon::from_bytes(LEXICON_FST.to_vec(), LEXICON_DAT.to_vec())?
        } else {
            Lexicon::from_static(LEXICON_FST, LEXICON_DAT)?
        };
        // Touch one lookup so the structure is real, not optimized away.
        let n = lex.analyze("Tisch").len();
        println!(
            "{mode}: loaded {} surfaces in {:.2} ms (probe Tisch -> {n})",
            lex.num_surfaces(),
            t.elapsed().as_micros() as f64 / 1000.0
        );
        return Ok(());
    }

    let passes: u32 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    let t_load = Instant::now();
    let lex = Lexicon::from_static(LEXICON_FST, LEXICON_DAT)?;
    let load_us = t_load.elapsed().as_micros();

    // Collect every surface form once.
    let map = FstMap::new(LEXICON_FST)?;
    let mut words: Vec<String> = Vec::with_capacity(lex.num_surfaces() as usize);
    let mut stream = map.stream();
    while let Some((key, _)) = stream.next() {
        words.push(String::from_utf8_lossy(key).into_owned());
    }

    // Warmup (also validates everything decodes).
    let mut checksum: u64 = 0;
    for w in &words {
        checksum = checksum.wrapping_add(lex.analyze(w).len() as u64);
    }

    // Timed passes.
    let mut analyses: u64 = 0;
    let t = Instant::now();
    for _ in 0..passes {
        for w in &words {
            let a = lex.analyze(w);
            analyses += a.len() as u64;
            // Touch the lemma so a borrowed Cow is actually dereferenced.
            if let Some(first) = a.first() {
                checksum = checksum.wrapping_add(first.lemma.as_bytes()[0] as u64);
            }
        }
    }
    let elapsed = t.elapsed();

    let calls = words.len() as u64 * passes as u64;
    let secs = elapsed.as_secs_f64();
    println!("lexicon: from_static  fst={} B  dat={} B", LEXICON_FST.len(), LEXICON_DAT.len());
    println!("load: {:.2} ms", load_us as f64 / 1000.0);
    println!("surfaces: {}   passes: {}   checksum: {}", words.len(), passes, checksum);
    println!(
        "analyze calls: {}   analyses: {}   elapsed: {:.3} s",
        calls, analyses, secs
    );
    println!(
        "throughput: {:.2} M calls/s   {:.1} ns/call   {:.1} ns/analysis",
        calls as f64 / secs / 1e6,
        secs * 1e9 / calls as f64,
        secs * 1e9 / analyses as f64,
    );
    Ok(())
}
