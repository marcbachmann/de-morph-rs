//! Library surface of the build tooling.
//!
//! The `de-morph-build` binary (`src/main.rs`) keeps its own module tree for
//! the lexicon build/package pipeline. This library re-exposes the Wiktionary
//! **extractors** as a public, reusable surface so downstream consumers — e.g.
//! a decompounder dictionary generator — can call them directly instead of
//! shelling out to the binary.
//!
//! Per-POS entry points: [`wiktionary::noun`] / [`wiktionary::verb`] /
//! [`wiktionary::adjective`] / [`wiktionary::compound`], plus
//! [`wiktionary::dump::PageReader`] for streaming a dump. [`extract_all`] runs
//! all four over one page in a single call.

pub mod wiktionary;

use wiktionary::compound::CompoundEntry;
use wiktionary::ExtractedEntry;

/// All decompounder-relevant extractions for one page, in a single call.
/// Lets a downstream consumer reuse the extractors without re-deriving the
/// per-template orchestration.
pub struct PageExtractions {
    pub nouns: Vec<ExtractedEntry>,
    pub verbs: Vec<ExtractedEntry>,
    pub adjectives: Vec<ExtractedEntry>,
    pub compounds: Vec<CompoundEntry>,
}

/// Run the noun, verb, adjective, and compound extractors over one page's
/// wikitext in a single call — the convenience entry point for downstream
/// reuse (e.g. a decompounder dictionary generator).
pub fn extract_all(title: &str, page_text: &str) -> PageExtractions {
    PageExtractions {
        nouns: wiktionary::noun::extract_nouns(title, page_text),
        verbs: wiktionary::verb::extract_verbs(title, page_text),
        adjectives: wiktionary::adjective::extract_adjectives(title, page_text),
        compounds: wiktionary::compound::extract_compounds(title, page_text),
    }
}
