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

/// Whether a template form cell holds a usable surface form.
///
/// Wiktionary marks an absent form — a non-comparable adjective's
/// comparative, a singulare-/pluraletantum's missing number — with a
/// dash placeholder (`-`, en/em dash, non-breaking hyphen) and sometimes
/// abbreviates a comparative as a bare suffix (`-ibler`). None of these
/// are real word forms. Feeding them to the paradigm generators seeds
/// phantom stems that inflate into junk surfaces (`-es`, `-en`, `-er`,
/// …), so every extractor drops such cells up front.
///
/// A real form must be non-empty after trimming, must contain at least
/// one alphabetic character, and must not begin with a dash (no German
/// word does — a leading dash always marks a placeholder or suffix).
pub fn is_real_form(form: &str) -> bool {
    let t = form.trim();
    !t.is_empty()
        && !t.starts_with(['-', '\u{2013}', '\u{2014}', '\u{2011}', '\u{2212}'])
        && t.chars().any(char::is_alphabetic)
}

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

#[cfg(test)]
mod tests {
    use super::is_real_form;

    #[test]
    fn is_real_form_rejects_placeholders_and_suffixes() {
        for bad in ["", "  ", "-", "—", "–", "-ibler", "-er", " — "] {
            assert!(!is_real_form(bad), "wrongly accepted {bad:?}");
        }
        for ok in ["reversibler", "liebte", "Tische", "1,2-Butadien", "groß"] {
            assert!(is_real_form(ok), "wrongly rejected {ok:?}");
        }
    }
}
