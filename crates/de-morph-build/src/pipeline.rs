//! `de-morph-build all` — the reproducible lexicon pipeline.
//!
//! Ports `scripts/build/lexicon.sh`:
//!   1. verify the raw dump sha256 against the pinned snapshot
//!   2. extract every POS to data/wiktionary/processed/*.jsonl
//!   3. build the FST + side table
//!   4. verify the lossless analysis fingerprint
//!
//! Unlike the old shell script there is no recompile dance: this binary
//! loads the freshly built lexicon from disk and dumps it directly.

use std::fs::File;
use std::io::{self, Read, Write};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::config;

pub fn run(args: &[String]) -> Result<()> {
    let mut skip_extract = false;
    for a in args {
        match a.as_str() {
            "--skip-extract" => skip_extract = true,
            "--help" | "-h" => {
                eprintln!("de-morph-build all [--skip-extract]");
                return Ok(());
            }
            other => bail!("unknown argument: {other}"),
        }
    }

    let dump = config::dump_path();

    // --- 1. verify the raw dump --------------------------------------------
    if skip_extract {
        println!("[1/4] --skip-extract: not verifying raw dump");
    } else {
        if !std::path::Path::new(&dump).exists() {
            bail!("raw dump missing: {dump}\n  run: bash scripts/fetch/dewiktionary.sh");
        }
        let actual = sha256_file(&dump).with_context(|| format!("hashing {dump}"))?;
        if actual != config::RAW_SHA256 {
            bail!(
                "raw dump hash mismatch\n  expected={}\n  got     ={actual}",
                config::RAW_SHA256
            );
        }
        println!("[1/4] raw dump verified  sha256={actual}");
    }

    // --- 2. extract --------------------------------------------------------
    if skip_extract {
        println!("[2/4] --skip-extract: reusing existing data/wiktionary/processed/*.jsonl");
    } else {
        std::fs::create_dir_all("data/wiktionary/processed")?;
        for kind in config::EXTRACTORS {
            println!("[2/4] extract {kind} ...");
            let out = format!("data/wiktionary/processed/{kind}.jsonl");
            crate::extract::run(&[
                kind.to_string(),
                "--input".into(),
                dump.clone(),
                "--output".into(),
                out,
            ])
            .with_context(|| format!("extracting {kind}"))?;
        }
    }

    // --- 3. build the lexicon ---------------------------------------------
    println!("[3/4] build ...");
    std::fs::create_dir_all("data/lexicon")?;
    crate::build_lexicon::run(&[
        "--fst-out".into(),
        config::FST_OUT.into(),
        "--dat-out".into(),
        config::DAT_OUT.into(),
    ])
    .context("building lexicon")?;

    // --- 4. verify the lossless fingerprint -------------------------------
    let dump_sha = fingerprint(config::FST_OUT, config::DAT_OUT)?;
    println!("[4/4] lossless analysis dump sha256={dump_sha}");
    let expected = std::env::var("EXPECTED_DUMP_SHA256")
        .unwrap_or_else(|_| config::EXPECTED_DUMP_SHA256.into());
    if !expected.is_empty() && dump_sha != expected {
        bail!(
            "lossless fingerprint changed\n  expected={expected}\n  got     ={dump_sha}\n\
             If this change is intentional, update EXPECTED_DUMP_SHA256 in de-morph-build/src/config.rs."
        );
    }

    let fst_len = std::fs::metadata(config::FST_OUT)?.len();
    let dat_len = std::fs::metadata(config::DAT_OUT)?.len();
    println!(
        "\nDone.\n  {}  ({fst_len} bytes)\n  {}  ({dat_len} bytes)",
        config::FST_OUT,
        config::DAT_OUT
    );
    println!("  Package for separate CC BY-SA distribution:\n    de-morph-build package");
    Ok(())
}

/// sha256 of the canonical analysis dump over a built lexicon — the
/// lossless fingerprint. Loads the lexicon from disk and streams the dump
/// straight into the hasher (no multi-hundred-MB intermediate buffer).
pub fn fingerprint(fst: &str, dat: &str) -> Result<String> {
    let lex = de_morph::Lexicon::open(fst, dat)
        .with_context(|| format!("opening lexicon {fst} / {dat}"))?;
    let mut hw = HashWriter(Sha256::new());
    lex.write_canonical_dump(&mut hw)?;
    Ok(hex(hw.0.finalize()))
}

/// sha256 of a file, streamed in chunks.
pub fn sha256_file(path: &str) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(hasher.finalize()))
}

fn hex(digest: impl AsRef<[u8]>) -> String {
    let mut s = String::with_capacity(64);
    for b in digest.as_ref() {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// `io::Write` adapter that feeds bytes into a `Sha256` instead of a sink.
struct HashWriter(Sha256);

impl Write for HashWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
