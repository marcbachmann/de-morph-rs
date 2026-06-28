//! `de-morph-build package` — stage the CC BY-SA 4.0 data bundle.
//!
//! Ports `scripts/build/package-data.sh`. The MIT crate ships no
//! Wiktionary-derived bytes; the lexicon travels as this standalone
//! tarball carrying its own licence, attribution, and provenance so the
//! CC BY-SA obligations stay with the data. Re-verifies the lossless
//! fingerprint before packaging so a stale artifact can never ship.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::config;
use crate::pipeline::{fingerprint, sha256_file};

const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run(args: &[String]) -> Result<()> {
    if let Some(a) = args.first() {
        if a == "--help" || a == "-h" {
            eprintln!("de-morph-build package");
            return Ok(());
        }
        bail!("unknown argument: {a}");
    }

    let fst = config::FST_OUT;
    let dat = config::DAT_OUT;
    if !Path::new(fst).exists() || !Path::new(dat).exists() {
        bail!("lexicon not built. Run: de-morph-build all");
    }

    // --- re-verify the lossless fingerprint before shipping ----------------
    println!("Verifying lossless fingerprint ...");
    let dump_sha = fingerprint(fst, dat)?;
    if dump_sha != config::EXPECTED_DUMP_SHA256 {
        bail!(
            "artifact does not match the pinned lossless fingerprint\n  \
             expected={}\n  got     ={dump_sha}\nRebuild with: de-morph-build all",
            config::EXPECTED_DUMP_SHA256
        );
    }
    println!("  ok  sha256={dump_sha}");

    let pkg = format!(
        "de-morph-lexicon-v{CRATE_VERSION}-dewiktionary-{}",
        config::DUMP_DATE
    );
    let stage = format!("dist/{pkg}");

    // --- stage the bundle --------------------------------------------------
    if Path::new(&stage).exists() {
        fs::remove_dir_all(&stage)?;
    }
    fs::create_dir_all(&stage)?;
    fs::copy(fst, format!("{stage}/lexicon.fst"))?;
    fs::copy(dat, format!("{stage}/lexicon.dat"))?;
    fs::copy("LICENSES/CC-BY-SA-4.0.txt", format!("{stage}/LICENSE"))
        .context("copying CC-BY-SA-4.0 licence text")?;

    fs::write(format!("{stage}/ATTRIBUTION"), attribution())?;
    fs::write(format!("{stage}/PROVENANCE"), provenance())?;
    fs::write(format!("{stage}/README.md"), readme())?;

    // SHA256SUMS over the two artifact files (sha256sum -c compatible).
    let mut sums = String::new();
    for name in ["lexicon.fst", "lexicon.dat"] {
        let h = sha256_file(&format!("{stage}/{name}"))?;
        sums.push_str(&format!("{h}  {name}\n"));
    }
    fs::write(format!("{stage}/SHA256SUMS"), sums)?;

    // --- tarball -----------------------------------------------------------
    let tarball = format!("dist/{pkg}.tar.gz");
    let status = Command::new("tar")
        .args(["-C", "dist", "-czf", &tarball, &pkg])
        .status()
        .context("running tar")?;
    if !status.success() {
        bail!("tar failed with status {status}");
    }
    let tar_sha = sha256_file(&tarball)?;
    fs::write(format!("{tarball}.sha256"), format!("{tar_sha}  {tarball}\n"))?;

    let tar_len = fs::metadata(&tarball)?.len();
    println!("\nPackaged CC BY-SA 4.0 data bundle:");
    println!("  {tarball}  ({tar_len} bytes)");
    println!("  staged tree: {stage}/");
    println!("\nReady to attach to a GitHub release or object store. The MIT");
    println!("crate ships none of these bytes.");
    Ok(())
}

fn attribution() -> String {
    let base = config::DUMP_URL.rsplit_once('/').map(|(a, _)| a).unwrap_or("");
    format!(
        "This artifact is a derivative of the German Wiktionary and is licensed under\n\
         the Creative Commons Attribution-ShareAlike 4.0 International licence\n\
         (CC BY-SA 4.0); see LICENSE.\n\n\
         Attribution (required by CC BY-SA 4.0, section 3(a)):\n\n\
         \x20   \"German Wiktionary contributors\", {base}/\n\
         \x20   Source: {url}\n\
         \x20   Article histories: https://de.wiktionary.org/\n\n\
         ShareAlike: if you remix, transform, or build upon this artifact, you must\n\
         distribute your contributions under CC BY-SA 4.0. This obligation attaches to\n\
         the data only; it does not extend to software that merely loads it.\n",
        url = config::DUMP_URL,
    )
}

fn provenance() -> String {
    format!(
        "de-morph lexicon — provenance\n\
         =============================\n\n\
         artifact          : lexicon.fst + lexicon.dat\n\
         on-disk format    : v{fmt} (de-morph-rs lexicon format)\n\
         built by          : de-morph-build v{ver}\n\n\
         source            : German Wiktionary (de.wiktionary.org)\n\
         snapshot          : {date}\n\
         dump url          : {url}\n\
         dump sha256       : {raw}\n\
         licence           : CC BY-SA 4.0 (see LICENSE, ATTRIBUTION)\n\n\
         lossless fingerprint (sha256 of the canonical analysis dump over all surfaces):\n\
         \x20                   {fp}\n\n\
         Reproduce from the dump:\n\
         \x20   bash scripts/fetch/dewiktionary.sh        # fetch + verify the snapshot\n\
         \x20   de-morph-build all                        # extract + build + verify\n\
         \x20   de-morph-build package                    # produce this bundle\n",
        fmt = config::FORMAT_VERSION,
        ver = CRATE_VERSION,
        date = config::DUMP_DATE,
        url = config::DUMP_URL,
        raw = config::RAW_SHA256,
        fp = config::EXPECTED_DUMP_SHA256,
    )
}

fn readme() -> String {
    format!(
        "# de-morph lexicon (data artifact)\n\n\
         Prebuilt morphological lexicon for [de-morph-rs][crate], derived from the\n\
         German Wiktionary snapshot `{date}`.\n\n\
         **Licence: CC BY-SA 4.0** (see `LICENSE` and `ATTRIBUTION`). This data\n\
         artifact is *separate* from the de-morph-rs crate, which is MIT-licensed and\n\
         contains none of these bytes. Using this artifact subjects the data — not your\n\
         code — to CC BY-SA's attribution and ShareAlike terms.\n\n\
         ## Files\n\n\
         | file          | description                                  |\n\
         |---------------|----------------------------------------------|\n\
         | `lexicon.fst` | surface → packed-pointer FST (format v{fmt})       |\n\
         | `lexicon.dat` | lemma table + packed analysis records        |\n\
         | `SHA256SUMS`  | checksums of the two files above             |\n\n\
         ## Use\n\n\
         ```rust\n\
         // Owns the bytes (e.g. read from disk at startup):\n\
         let lex = de_morph::Lexicon::from_bytes(fst_bytes, dat_bytes)?;\n\n\
         // Or zero-copy from 'static bytes (e.g. include_bytes! in your own,\n\
         // CC-BY-SA-aware, binary):\n\
         let lex = de_morph::Lexicon::from_static(FST, DAT)?;\n\
         ```\n\n\
         Verify integrity: `sha256sum -c SHA256SUMS`.\n\n\
         [crate]: https://crates.io/crates/de-morph-rs\n",
        date = config::DUMP_DATE,
        fmt = config::FORMAT_VERSION,
    )
}
