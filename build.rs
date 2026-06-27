//! Stage the embedded lexicon for the `de-morph` binary.
//!
//! The runtime subcommands embed `data/lexicon/lexicon.{fst,dat}` via
//! `include_bytes!`. Those artifacts are gitignored and regenerable, so a
//! fresh checkout has none — yet `de-morph` is also the tool that *builds*
//! the lexicon (the `build-lexicon` / `extract` subcommands, gated behind
//! the `extractor` feature). To break that bootstrap cycle we copy the
//! lexicon into `OUT_DIR` when it exists and write an empty placeholder
//! when it doesn't, so the crate always compiles. Once the lexicon is
//! built, the `rerun-if-changed` lines below pick it up on the next build.

use std::path::Path;

fn main() {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR set by cargo");
    for name in ["lexicon.fst", "lexicon.dat"] {
        let src = Path::new("data/lexicon").join(name);
        let dst = Path::new(&out_dir).join(name);
        println!("cargo:rerun-if-changed=data/lexicon/{name}");
        if src.exists() {
            std::fs::copy(&src, &dst).unwrap_or_else(|e| panic!("copy {name}: {e}"));
        } else if !dst.exists() {
            // Empty placeholder so include_bytes! resolves. Runtime load
            // of an empty lexicon fails gracefully; the extractor/build
            // subcommands never touch it.
            std::fs::write(&dst, []).unwrap_or_else(|e| panic!("stub {name}: {e}"));
        }
    }
}
