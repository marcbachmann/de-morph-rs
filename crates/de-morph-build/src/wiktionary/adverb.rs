//! Extract German adverb lemmas from a Wiktionary page.
//!
//! Most German adverbs are morphologically uninflected ("heute",
//! "gestern", "oft", "sehr", "dort"), so the extractor's job is small:
//! detect that the page has a German Adverb section and emit one
//! entry per such section, using the page title as the lemma.
//!
//! Some adverbs can be compared (schnell/schneller/am schnellsten),
//! but those forms also surface as the corresponding adjective's
//! comparative/superlative — they're already in the lexicon via the
//! adjective extractor. The adverb extractor here emits only the
//! uninflected base form.
//!
//! References (verified):
//! - Wortart/Adverb template documentation:
//!   <https://de.wiktionary.org/wiki/Vorlage:Wortart>
//! - German adverb-section convention: any Wiktionary entry whose
//!   POS heading contains `{{Wortart|Adverb|Deutsch}}` is treated as
//!   having a German adverb sense.

use de_morph::analysis::{Features, PronType, Source, UPOS};

use crate::wiktionary::ExtractedEntry;

/// Return one `ExtractedEntry` per German adverb section found in
/// `page_text`. The page title is taken as the lemma.
///
/// If the page is a Pronominaladverb (`{{Wortart|Pronominaladverb|Deutsch}}`,
/// e.g. *worüber*, *darüber*, *womit*, *damit*), the entry is tagged
/// with the appropriate [`PronType`]:
/// - `wo-` / `wor-` prefix → [`PronType::Int`] (interrogative; doubles
///   as the relative form, which UD tags contextually).
/// - `da-` / `dar-` / `hier-` prefix → [`PronType::Dem`] (demonstrative).
///
/// The detection is simple substring matching — fast and correct for
/// the canonical template usage.
pub fn extract_adverbs(title: &str, page_text: &str) -> Vec<ExtractedEntry> {
    let is_pron = has_german_pronominaladverb_section(page_text);
    let is_plain = has_german_adverb_section(page_text);
    if !is_pron && !is_plain {
        return Vec::new();
    }
    // PronType is set when EITHER:
    //   1. the page explicitly carries the Pronominaladverb wortart, OR
    //   2. the lemma matches the wo-/wor-/da-/dar-/hier- + preposition
    //      pattern (Wiktionary uses the Pronominaladverb heading
    //      inconsistently — most PAs sit under plain `Adverb`).
    let mut features = Features::empty();
    if is_pron || is_plain {
        features.pron_type = pronominal_adverb_type(title);
    }
    vec![ExtractedEntry {
        surface: title.to_string(),
        lemma: title.to_string(),
        pos: UPOS::ADV,
        features,
        source: Source::Attested,
        source_title: title.to_string(),
    }]
}

/// Classify a pronominal adverb by its lemma. German has three productive
/// families, all formed from a pro-form (`wo`/`da`/`hier`) plus a
/// preposition:
///
/// - `wor`- + vowel-initial prep → [`PronType::Int`] (woran, worüber,
///   worauf, worin, worum, worunter)
/// - `wo`- + consonant-initial prep → [`PronType::Int`] (womit, woher,
///   wohin, wodurch, wofür, wozu, wonach, wovon, wovor, wogegen, wobei)
/// - `dar`- / `da`- → [`PronType::Dem`] (mirrors the wo-/wor- pattern:
///   daran, darüber, damit, daher, ...)
/// - `hier`- → [`PronType::Dem`] (hierauf, hierin, hiermit, hierdurch,
///   hierzu, ...)
///
/// The detection uses a strict preposition allow-list rather than a
/// bare prefix check so that plain adverbs like `wohl`, `damals`,
/// `dann`, `dort` aren't false-positive tagged. Wiktionary's
/// `{{Wortart|Pronominaladverb|Deutsch}}` heading is only emitted on a
/// minority of pages — most pronominal adverbs sit under plain
/// `{{Wortart|Adverb|Deutsch}}`, so we extend the rule lemma-side.
fn pronominal_adverb_type(lemma: &str) -> Option<PronType> {
    // Bare wor- / dar- are always pronominal — no real German lemma
    // outside this family starts with these three-letter prefixes.
    if let Some(rest) = lemma.strip_prefix("wor") {
        if !rest.is_empty() && starts_with_vowel(rest) {
            return Some(PronType::Int);
        }
    }
    if let Some(rest) = lemma.strip_prefix("dar") {
        if !rest.is_empty() && starts_with_vowel(rest) {
            return Some(PronType::Dem);
        }
    }
    // hier- + preposition. The bare adverb "hier" stays untagged (it's
    // locative, not pronominal); we require a suffix from the preposition
    // allow-list.
    if let Some(suffix) = lemma.strip_prefix("hier") {
        if PA_PREP_SUFFIXES.contains(&suffix) {
            return Some(PronType::Dem);
        }
    }
    // wo-/da- + consonant-initial preposition. Strict allow-list keeps
    // wohl, damals, dann, dort, dorthin from false-positiving.
    if let Some(suffix) = lemma.strip_prefix("wo") {
        if PA_PREP_SUFFIXES.contains(&suffix) {
            return Some(PronType::Int);
        }
    }
    if let Some(suffix) = lemma.strip_prefix("da") {
        if PA_PREP_SUFFIXES.contains(&suffix) {
            return Some(PronType::Dem);
        }
    }
    None
}

/// Preposition suffixes that combine with `wo-`/`da-`/`hier-` to form a
/// German pronominal adverb. Curated from the Universal Dependencies
/// PronType=Int/Dem inventory and Wiktionary's pronominal-adverb category.
const PA_PREP_SUFFIXES: &[&str] = &[
    "bei", "durch", "für", "gegen", "her", "hin", "hinter", "mit", "nach", "neben", "von", "vor",
    "zu", "zwischen",
];

fn starts_with_vowel(s: &str) -> bool {
    matches!(
        s.chars().next(),
        Some('a' | 'e' | 'i' | 'o' | 'u' | 'ä' | 'ö' | 'ü')
    )
}

/// Detect whether the wikitext has at least one German adverb section.
///
/// Matches the standard `{{Wortart|Adverb|Deutsch}}` template
/// invocation. Variations with intervening whitespace are rare; if a
/// page slips through, we miss one adverb — not a correctness disaster.
fn has_german_adverb_section(page_text: &str) -> bool {
    if !page_text.contains("Adverb") {
        return false;
    }
    page_text.contains("{{Wortart|Adverb|Deutsch}}")
}

/// Detect whether the wikitext has a German Pronominaladverb section
/// (`worüber`, `damit`, `wozu`, etc.).
fn has_german_pronominaladverb_section(page_text: &str) -> bool {
    if !page_text.contains("Pronominaladverb") {
        return false;
    }
    page_text.contains("{{Wortart|Pronominaladverb|Deutsch}}")
}

/// Particle template variants that Wiktionary uses for German.
/// Counts from the 20260601 dump: ~44 "Partikel", 18 "Gradpartikel",
/// 18 "Antwortpartikel", 9 "Modalpartikel", 7 "Fokuspartikel" =
/// ~96 entries total.
const PARTICLE_TEMPLATES: &[&str] = &[
    "{{Wortart|Partikel|Deutsch}}",
    "{{Wortart|Gradpartikel|Deutsch}}",
    "{{Wortart|Antwortpartikel|Deutsch}}",
    "{{Wortart|Modalpartikel|Deutsch}}",
    "{{Wortart|Fokuspartikel|Deutsch}}",
    "{{Wortart|Negationspartikel|Deutsch}}",
];

/// Extract German particle lemmas (UPOS::PART) from a Wiktionary page.
///
/// A page may have multiple particle sub-types (e.g. some particles
/// appear under both Modalpartikel and Fokuspartikel). We emit only
/// ONE entry per page — the lemma is the page title and the analysis
/// is `(UPOS::PART, Features::empty())`. The particle's sub-type
/// (Modal / Fokus / Antwort / Grad) is not recorded as a feature,
/// since the Features struct has no dedicated slot for it.
pub fn extract_particles(title: &str, page_text: &str) -> Vec<ExtractedEntry> {
    if !has_german_particle_section(page_text) {
        return Vec::new();
    }
    vec![ExtractedEntry {
        surface: title.to_string(),
        lemma: title.to_string(),
        pos: UPOS::PART,
        features: Features::empty(),
        source: Source::Attested,
        source_title: title.to_string(),
    }]
}

fn has_german_particle_section(page_text: &str) -> bool {
    if !page_text.contains("partikel") && !page_text.contains("Partikel") {
        return false;
    }
    PARTICLE_TEMPLATES.iter().any(|t| page_text.contains(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_simple_particle() {
        let text = "== nicht ({{Sprache|Deutsch}}) ==\n\
            === {{Wortart|Partikel|Deutsch}}, {{Wortart|Negationspartikel|Deutsch}} ===";
        let entries = extract_particles("nicht", text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pos, UPOS::PART);
        assert_eq!(entries[0].lemma, "nicht");
    }

    #[test]
    fn modalpartikel_template_picked_up() {
        let text = "=== {{Wortart|Modalpartikel|Deutsch}} ===";
        let entries = extract_particles("doch", text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pos, UPOS::PART);
    }

    #[test]
    fn page_without_particle_section_yields_nothing() {
        let text = "=== {{Wortart|Substantiv|Deutsch}} ===";
        let entries = extract_particles("Tisch", text);
        assert!(entries.is_empty());
    }

    #[test]
    fn extracts_simple_adverb() {
        let text = "== heute ({{Sprache|Deutsch}}) ==\n\
            === {{Wortart|Adverb|Deutsch}} ===\n\
            Adverb der Zeit.";
        let entries = extract_adverbs("heute", text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].surface, "heute");
        assert_eq!(entries[0].lemma, "heute");
        assert_eq!(entries[0].pos, UPOS::ADV);
        assert_eq!(entries[0].source, Source::Attested);
    }

    #[test]
    fn page_without_adverb_section_yields_nothing() {
        let text = "== Tisch ({{Sprache|Deutsch}}) ==\n\
            === {{Wortart|Substantiv|Deutsch}}, {{m}} ===";
        let entries = extract_adverbs("Tisch", text);
        assert!(entries.is_empty());
    }

    #[test]
    fn page_with_mention_but_no_adverb_section_yields_nothing() {
        let text = "== Tisch ({{Sprache|Deutsch}}) ==\n\
            siehe auch [[Adverb]]";
        let entries = extract_adverbs("Tisch", text);
        assert!(entries.is_empty());
    }

    #[test]
    fn other_language_adverb_section_yields_nothing() {
        let text = "== fast ({{Sprache|Englisch}}) ==\n\
            === {{Wortart|Adverb|Englisch}} ===";
        let entries = extract_adverbs("fast", text);
        assert!(entries.is_empty());
    }

    #[test]
    fn worueber_tagged_as_int_pronominaladverb() {
        let text = "== worüber ({{Sprache|Deutsch}}) ==\n\
            === {{Wortart|Pronominaladverb|Deutsch}} ===";
        let entries = extract_adverbs("worüber", text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pos, UPOS::ADV);
        assert_eq!(entries[0].features.pron_type, Some(PronType::Int));
    }

    #[test]
    fn darueber_tagged_as_dem_pronominaladverb() {
        let text = "== darüber ({{Sprache|Deutsch}}) ==\n\
            === {{Wortart|Pronominaladverb|Deutsch}} ===";
        let entries = extract_adverbs("darüber", text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pos, UPOS::ADV);
        assert_eq!(entries[0].features.pron_type, Some(PronType::Dem));
    }

    #[test]
    fn hierdurch_tagged_as_dem_pronominaladverb() {
        let text = "=== {{Wortart|Pronominaladverb|Deutsch}} ===";
        let entries = extract_adverbs("hierdurch", text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].features.pron_type, Some(PronType::Dem));
    }

    #[test]
    fn pronominaladverb_collapses_concurrent_plain_adverb_heading() {
        // Some pages list both wortart variants on the same page —
        // emit ONE entry tagged as the pronominal variant rather than
        // duplicating a bare ADV without PronType.
        let text = "== womit ({{Sprache|Deutsch}}) ==\n\
            === {{Wortart|Adverb|Deutsch}}, {{Wortart|Pronominaladverb|Deutsch}} ===";
        let entries = extract_adverbs("womit", text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].features.pron_type, Some(PronType::Int));
    }

    #[test]
    fn plain_adverb_gets_no_prontype() {
        let text = "=== {{Wortart|Adverb|Deutsch}} ===";
        let entries = extract_adverbs("heute", text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].features.pron_type, None);
    }
}
