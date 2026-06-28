//! `de-morph bench` — throughput + memory benchmark for the runtime analyzer.
//!
//! Loads the lexicon from disk, collects every surface form, then times
//! `Lexicon::analyze` over all of them across several passes. Run under
//! `/usr/bin/time -l` to capture max RSS:
//!
//!   cargo build --release --bin de-morph
//!   /usr/bin/time -l ./target/release/de-morph bench 5
//!
//! Modes (arg 1):
//!   sweep [passes]   throughput over every surface (default, passes=5)
//!   load             read from disk + `from_static` (leaked) load only
//!   loadbytes        read from disk + `from_bytes` load only
//! Combine with `/usr/bin/time -l` to read max RSS per mode.

use std::time::Instant;

use de_morph::Lexicon;
use fst::{Map as FstMap, Streamer};

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mode = args.first().cloned().unwrap_or_else(|| "sweep".into());

    // The lexicon is read from disk (the binary embeds nothing). The two
    // load modes still contrast the constructors: `from_bytes` owns the
    // heap Vec; `from_static` borrows it (here via a leaked Box, since the
    // bytes come from a file rather than the binary image).
    if mode == "load" || mode == "loadbytes" {
        let (fst, dat) = crate::loader::read_bytes()?;
        let t = Instant::now();
        let lex = if mode == "loadbytes" {
            Lexicon::from_bytes(fst, dat)?
        } else {
            Lexicon::from_static(Box::leak(fst.into_boxed_slice()), Box::leak(dat.into_boxed_slice()))?
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

    let passes: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5);

    let (fst_bytes, dat_bytes) = crate::loader::read_bytes()?;
    let (fst_len, dat_len) = (fst_bytes.len(), dat_bytes.len());
    let t_load = Instant::now();
    let lex = Lexicon::from_bytes(fst_bytes.clone(), dat_bytes)?;
    let load_us = t_load.elapsed().as_micros();

    // Collect every surface form once.
    let map = FstMap::new(fst_bytes)?;
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
    println!("lexicon: from_bytes  fst={fst_len} B  dat={dat_len} B");
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
