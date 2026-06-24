//! Wiktionary extraction pipeline (build-time tooling).
//!
//! The submodules in here read a German Wiktionary dump
//! (`*-pages-articles.xml.bz2`) and emit `(lemma, pos, features, surface
//! form)` records that feed the runtime FST. None of this code is reachable
//! from the library's default build — the module is gated behind the
//! `extractor` cargo feature.
//!
//! References:
//! - Wiktionary dump format: <https://meta.wikimedia.org/wiki/Data_dumps>
//! - MediaWiki XML export schema:
//!   <https://www.mediawiki.org/wiki/Help:Export>
//! - Wiktionary noun template family ({{Deutsch Substantiv Übersicht}}):
//!   <https://de.wiktionary.org/wiki/Vorlage:Deutsch_Substantiv_Übersicht>
//! - CC BY-SA 4.0 attribution requirements:
//!   <https://creativecommons.org/licenses/by-sa/4.0/> and
//!   <https://foundation.wikimedia.org/wiki/Policy:Terms_of_Use> § 7.

pub mod abbreviation;
pub mod adjective;
pub mod adverb;
pub mod compound;
pub mod dump;
pub mod noun;
pub mod pronoun;
pub mod propn;
pub mod template;
pub mod verb;

pub use dump::{Page, PageReader};
pub use template::{find_templates, parse_template_body, Template};

use crate::analysis::{Features, Source, UPOS};

/// One extracted (surface form, analysis) record with provenance back
/// to the source Wiktionary article. Shared between the noun and verb
/// extractors (and future adjective / pronoun extractors).
///
/// `source` distinguishes lexicon-attested forms (`Source::Attested`)
/// from forms produced by paradigm rules during build-time expansion
/// (`Source::Inflected`). The two categories share the JSONL output
/// shape so downstream tooling can tell them apart by tag rather than
/// by file.
///
/// `source_title` is the Wiktionary article title that originated the
/// lemma, required for CC BY-SA attribution downstream
/// (<https://foundation.wikimedia.org/wiki/Policy:Terms_of_Use> § 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedEntry {
    pub surface: String,
    pub lemma: String,
    pub pos: UPOS,
    pub features: Features,
    pub source: Source,
    pub source_title: String,
}
