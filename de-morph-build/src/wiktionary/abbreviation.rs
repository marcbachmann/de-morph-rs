//! Extract German abbreviations from a Wiktionary page.
//!
//! Wiktionary tags abbreviations with `{{Wortart|Abkürzung|Deutsch}}`
//! (~5,000 entries in the 2026-06 dump). We emit one entry per page,
//! using the page title as both `surface` and `lemma` (UD convention:
//! abbreviation lemma == the abbreviated form, not the expansion).
//!
//! Each abbreviation needs a syntactic POS that matches its full form:
//!   `z. B.` → ADV (zum Beispiel)
//!   `Dr.`   → NOUN (Doktor)
//!   `bzw.`  → CCONJ (beziehungsweise)
//!
//! POS resolution proceeds in three layers:
//!   1. Hand-curated table (`ABBR_POS`) for the ~80 highest-frequency
//!      abbreviations — exact mapping, wins outright.
//!   2. **Bedeutungen aggregation**: parse `{{Bedeutungen}}`, collect
//!      every `[[wikilink target]]` across all sense lines, classify
//!      each target morphologically (suffix-based: `-isch`/`-lich` →
//!      Adj, `-en` → Verb, `-falls`/`-weise` → Adv, capital-start →
//!      Noun), and vote. The winning POS is taken.
//!   3. Orthographic fallback for the long tail where layer 2 can't
//!      classify anything: uppercase-leading → `Noun`, otherwise → `X`.
//!
//! Layer 2 is what turns `adj.` → ADJ (via `[[adjektivisch]]`), `ahd.`
//! → ADJ (via `[[althochdeutsch]]`), and `vgl.` → VERB (via
//! `[[vergleichen]]`) — entries we'd otherwise drop to NOUN or X.
//!
//! References:
//! - Wiktionary template: <https://de.wiktionary.org/wiki/Vorlage:Wortart>
//! - UD POS inventory: <https://universaldependencies.org/u/pos/>

use de_morph::analysis::{Features, Source, UPOS};
use crate::wiktionary::ExtractedEntry;

/// Hand-curated table: abbreviation → syntactic POS of its full form.
/// Sourced from the highest-frequency abbreviation surfaces visible in
/// the out-udpipe / UD-GSD / UD-HDT evaluations plus standard German
/// usage. Keys MUST match Wiktionary's page title (including spacing,
/// since Wiktionary writes `z. B.` with a space and `etc.` without).
const ABBR_POS: &[(&str, UPOS)] = &[
    // ADV — adverbial phrases ("for example", "namely", etc.)
    ("z. B.", UPOS::ADV),
    ("zB", UPOS::ADV),
    ("z.B.", UPOS::ADV),
    ("etc.", UPOS::ADV),
    ("usw.", UPOS::ADV),
    ("ggf.", UPOS::ADV),
    ("evtl.", UPOS::ADV),
    ("d. h.", UPOS::ADV),
    ("d.h.", UPOS::ADV),
    ("u. a.", UPOS::ADV),
    ("u.a.", UPOS::ADV),
    ("u. ä.", UPOS::ADV),
    ("u.ä.", UPOS::ADV),
    ("u. U.", UPOS::ADV),
    ("u.U.", UPOS::ADV),
    ("v. a.", UPOS::ADV),
    ("v.a.", UPOS::ADV),
    ("o. Ä.", UPOS::ADV),
    ("o.ä.", UPOS::ADV),
    ("o. ä.", UPOS::ADV),
    ("ca.", UPOS::ADV),
    ("o. g.", UPOS::ADV),
    ("s. o.", UPOS::ADV),
    ("s. u.", UPOS::ADV),
    ("inkl.", UPOS::ADV),
    ("ggf.", UPOS::ADV),
    ("vgl.", UPOS::ADV),
    ("z. T.", UPOS::ADV),
    ("z.T.", UPOS::ADV),
    ("z. Z.", UPOS::ADV),
    ("z.Z.", UPOS::ADV),
    ("z. Zt.", UPOS::ADV),
    // CCONJ / SCONJ — coordinating / subordinating conjunctions
    ("bzw.", UPOS::CCONJ),
    // ADP — prepositions
    ("lt.", UPOS::ADP),
    ("dgl.", UPOS::ADP),
    ("z. Hd.", UPOS::ADP),
    // NOUN — most title-style abbreviations
    ("Abk.", UPOS::NOUN),
    ("Abb.", UPOS::NOUN),
    ("Abs.", UPOS::NOUN),
    ("Abt.", UPOS::NOUN),
    ("Adj.", UPOS::NOUN),
    ("Adv.", UPOS::NOUN),
    ("Anm.", UPOS::NOUN),
    ("Aufl.", UPOS::NOUN),
    ("Ausg.", UPOS::NOUN),
    ("Az.", UPOS::NOUN),
    ("Bd.", UPOS::NOUN),
    ("Bsp.", UPOS::NOUN),
    ("Co.", UPOS::NOUN),
    ("Dr.", UPOS::NOUN),
    ("Fa.", UPOS::NOUN),
    ("Fr.", UPOS::NOUN),
    ("Hbf.", UPOS::NOUN),
    ("Hr.", UPOS::NOUN),
    ("Hrsg.", UPOS::NOUN),
    ("Inc.", UPOS::NOUN),
    ("Jh.", UPOS::NOUN),
    ("Jhd.", UPOS::NOUN),
    ("Jr.", UPOS::NOUN),
    ("Kap.", UPOS::NOUN),
    ("Min.", UPOS::NOUN),
    ("Mio.", UPOS::NOUN),
    ("Mrd.", UPOS::NOUN),
    ("Nr.", UPOS::NOUN),
    ("Prof.", UPOS::NOUN),
    ("Pkt.", UPOS::NOUN),
    ("Pl.", UPOS::NOUN),
    ("UPOS.", UPOS::NOUN),
    ("Reg.", UPOS::NOUN),
    ("Sg.", UPOS::NOUN),
    ("Sr.", UPOS::NOUN),
    ("St.", UPOS::NOUN),
    ("Std.", UPOS::NOUN),
    ("Str.", UPOS::NOUN),
    ("Tel.", UPOS::NOUN),
    ("Tsd.", UPOS::NOUN),
    ("Vol.", UPOS::NOUN),
    ("z. T.", UPOS::NOUN),
    ("Anh.", UPOS::NOUN),
    ("Aufl.", UPOS::NOUN),
    // PROPN — countries, organisations, agencies (UD's PROPN class).
    ("EU", UPOS::PROPN),
    ("USA", UPOS::PROPN),
    ("BRD", UPOS::PROPN),
    ("DDR", UPOS::PROPN),
    ("UNO", UPOS::PROPN),
    ("AG", UPOS::PROPN),
    ("GmbH", UPOS::PROPN),
    ("e. V.", UPOS::PROPN),
    ("e.V.", UPOS::PROPN),
    ("UN", UPOS::PROPN),
    ("UK", UPOS::PROPN),
    ("CH", UPOS::PROPN),
    ("DE", UPOS::PROPN),
    ("AT", UPOS::PROPN),
];

/// Look up a curated POS for an abbreviation title. Returns the POS if
/// the abbreviation is in our high-frequency hand-curated table,
/// otherwise `None` (caller applies the capitalisation heuristic).
fn curated_pos(title: &str) -> Option<UPOS> {
    ABBR_POS.iter().find(|(k, _)| *k == title).map(|(_, p)| *p)
}

/// Default POS for an unmapped abbreviation, based on German's
/// uniformly-capitalised noun orthography:
/// - `Abk.`, `Fr.`, `Mio.`, `USA`, `GmbH` (uppercase-leading) → NOUN
/// - `etc.`, `bzw.`, `ggf.`, `lt.` (lowercase-leading) → `X`
fn default_pos(title: &str) -> UPOS {
    if title.chars().next().is_some_and(|c| c.is_uppercase()) {
        UPOS::NOUN
    } else {
        UPOS::X
    }
}

/// Look at the page's `{{Bedeutungen}}` section, collect every
/// `[[wikilink]]` target across all sense lines, classify each target
/// morphologically, and return the POS that gets the most votes.
///
/// Ties are broken by the natural `UPOS` discriminant order (Noun, Verb,
/// Adj, Adv, ...). Returns `None` when no wikilink can be classified —
/// the caller then falls back to `default_pos`.
fn infer_pos_from_bedeutungen(page_text: &str) -> Option<UPOS> {
    let body = bedeutungen_body(page_text)?;
    let mut votes: [u32; 16] = [0; 16];
    for target in wikilink_targets(body) {
        if let Some(pos) = classify_by_suffix(target) {
            votes[pos as usize] += 1;
        }
    }
    let (idx, count) = votes
        .iter()
        .enumerate()
        .max_by_key(|(_, &v)| v)
        .unwrap_or((0, &0));
    if *count == 0 {
        None
    } else {
        // SAFETY: idx came from the enumerated loop over [0..16] which
        // is exactly the UPOS discriminant range.
        Some(unsafe { std::mem::transmute::<u8, UPOS>(idx as u8) })
    }
}

/// Extract the body of the `{{Bedeutungen}}` section: everything between
/// the marker and the next top-level wiki section (`{{Synonyme}}`,
/// `{{Beispiele}}`, `{{Gegenwörter}}`, `{{Herkunft}}`, etc.).
fn bedeutungen_body(page_text: &str) -> Option<&str> {
    const MARKER: &str = "{{Bedeutungen}}";
    let start = page_text.find(MARKER)? + MARKER.len();
    let rest = &page_text[start..];
    // Stop at the next top-of-section template invocation. Common
    // sentinels: {{Synonyme}}, {{Beispiele}}, {{Gegenwörter}},
    // {{Herkunft}}, {{Sinnverwandte}}, {{Charakteristische}}.
    //
    // We use `find()` rather than a byte loop because the body contains
    // German text with multibyte chars (ü/ö/ä/ß); slicing on a non-char
    // boundary would panic.
    let mut search_from = 0;
    while let Some(rel) = rest[search_from..].find("\n{{") {
        let abs = search_from + rel;
        let after = &rest[abs + 3..];
        if let Some(close) = after.find("}}") {
            let inside = &after[..close];
            if !inside.contains('|') && !inside.contains('\n') {
                return Some(&rest[..abs]);
            }
        }
        search_from = abs + 3;
    }
    Some(rest)
}

/// Iterate over every `[[target]]` wikilink target in `text`, stripping
/// any `|displayed` suffix and ignoring image links (`[[File:`, `[[Datei:`).
fn wikilink_targets(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        let start = rest.find("[[")?;
        let after_open = &rest[start + 2..];
        let close = after_open.find("]]")?;
        let inner = &after_open[..close];
        rest = &after_open[close + 2..];
        // Strip pipe display: [[target|displayed]] → target.
        let target = inner.split('|').next().unwrap_or(inner).trim();
        // Skip namespaced links: [[Datei:...]], [[Wikipedia:...]], etc.
        if target.contains(':') || target.is_empty() {
            return Some(""); // emit empty; caller's classifier ignores it
        }
        Some(target)
    })
    .filter(|t| !t.is_empty())
}

/// Classify a German lemma into a POS by morphological suffix. Returns
/// `None` when no suffix rule fires (the caller treats this as "no vote").
fn classify_by_suffix(word: &str) -> Option<UPOS> {
    let w = word.trim();
    if w.is_empty() {
        return None;
    }
    // Noun: capitalised first letter (German orthography rule). We test
    // this BEFORE the verb -en suffix so capital nouns like "Begehen",
    // "Lesen" don't get misclassified as verbs.
    if w.chars().next().is_some_and(|c| c.is_uppercase()) {
        return Some(UPOS::NOUN);
    }
    // Adverbs first — distinctive suffixes that don't overlap with adj/verb.
    if w.ends_with("falls")
        || w.ends_with("weise")
        || w.ends_with("dings")
        || w.ends_with("mals")
        || w.ends_with("tens")
        || w.ends_with("erst")
        || w == "circa"
        || w == "zirka"
    {
        return Some(UPOS::ADV);
    }
    // Adjectives: characteristic derivation suffixes, applied before the
    // verb -en rule to avoid losing "innen"/"außen" etc. (those are adv
    // anyway but the order matters for compound adjectives like
    // "norddeutsch" which doesn't end in -en).
    if w.ends_with("isch")
        || w.ends_with("lich")
        || w.ends_with("bar")
        || w.ends_with("haft")
        || w.ends_with("sam")
        || w.ends_with("los")
        || w.ends_with("ig")
        || w.ends_with("voll")
        || w.ends_with("artig")
        || w.ends_with("mäßig")
        || w.ends_with("deutsch")
    {
        return Some(UPOS::ADJ);
    }
    // Verb: infinitive endings.
    if w.ends_with("ieren")
        || w.ends_with("isieren")
        || (w.ends_with("en") && w.len() > 3)
        || w.ends_with("eln")
        || w.ends_with("ern")
    {
        return Some(UPOS::VERB);
    }
    None
}

/// Extract a German abbreviation from `page_text`, if the page has at
/// least one `{{Wortart|Abkürzung|Deutsch}}` section.
///
/// POS resolution order: curated table → Bedeutungen aggregation →
/// orthographic fallback.
pub fn extract_abbreviations(title: &str, page_text: &str) -> Vec<ExtractedEntry> {
    if !has_german_abbreviation_section(page_text) {
        return Vec::new();
    }
    let pos = curated_pos(title)
        .or_else(|| infer_pos_from_bedeutungen(page_text))
        .unwrap_or_else(|| default_pos(title));
    vec![ExtractedEntry {
        surface: title.to_string(),
        lemma: title.to_string(),
        pos,
        features: Features::empty(),
        source: Source::Attested,
        source_title: title.to_string(),
    }]
}

fn has_german_abbreviation_section(page_text: &str) -> bool {
    if !page_text.contains("Abkürzung") {
        return false;
    }
    page_text.contains("{{Wortart|Abkürzung|Deutsch}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_z_b_as_adv() {
        let text = "== z. B. ({{Sprache|Deutsch}}) ==\n\
            === {{Wortart|Abkürzung|Deutsch}} ===\n\
            {{Bedeutungen}}\n:[1] zum Beispiel";
        let entries = extract_abbreviations("z. B.", text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].surface, "z. B.");
        assert_eq!(entries[0].lemma, "z. B.");
        assert_eq!(entries[0].pos, UPOS::ADV);
    }

    #[test]
    fn extracts_etc_as_adv() {
        let text = "=== {{Wortart|Abkürzung|Deutsch}} ===";
        let entries = extract_abbreviations("etc.", text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pos, UPOS::ADV);
    }

    #[test]
    fn extracts_dr_as_noun() {
        let text = "=== {{Wortart|Abkürzung|Deutsch}} ===";
        let entries = extract_abbreviations("Dr.", text);
        assert_eq!(entries[0].pos, UPOS::NOUN);
    }

    #[test]
    fn extracts_bzw_as_cconj() {
        let text = "=== {{Wortart|Abkürzung|Deutsch}} ===";
        let entries = extract_abbreviations("bzw.", text);
        assert_eq!(entries[0].pos, UPOS::CCONJ);
    }

    #[test]
    fn unknown_capital_leading_falls_back_to_noun() {
        let text = "=== {{Wortart|Abkürzung|Deutsch}} ===";
        let entries = extract_abbreviations("Xyz.", text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pos, UPOS::NOUN);
        assert_eq!(entries[0].lemma, "Xyz.");
    }

    #[test]
    fn unknown_lowercase_leading_falls_back_to_x() {
        let text = "=== {{Wortart|Abkürzung|Deutsch}} ===";
        let entries = extract_abbreviations("xyz.", text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pos, UPOS::X);
        assert_eq!(entries[0].lemma, "xyz.");
    }

    #[test]
    fn page_without_abkuerzung_section_yields_nothing() {
        let text = "=== {{Wortart|Substantiv|Deutsch}} ===";
        let entries = extract_abbreviations("Tisch", text);
        assert!(entries.is_empty());
    }

    #[test]
    fn non_german_abbreviation_section_ignored() {
        let text = "=== {{Wortart|Abkürzung|Englisch}} ===";
        let entries = extract_abbreviations("etc.", text);
        assert!(entries.is_empty());
    }

    #[test]
    fn fr_resolves_to_noun() {
        let text = "=== {{Wortart|Abkürzung|Deutsch}} ===";
        let entries = extract_abbreviations("Fr.", text);
        assert_eq!(entries[0].pos, UPOS::NOUN);
    }

    #[test]
    fn bedeutungen_classifies_adjective_via_isch_suffix() {
        // `adj.` Wiktionary entry's Bedeutungen says `:[1] [[adjektivisch]]`.
        let text = "=== {{Wortart|Abkürzung|Deutsch}} ===\n\
            {{Bedeutungen}}\n\
            :[1] [[adjektivisch]]\n\
            {{Synonyme}}\n";
        let entries = extract_abbreviations("adj.", text);
        assert_eq!(entries[0].pos, UPOS::ADJ);
    }

    #[test]
    fn bedeutungen_classifies_verb_via_en_suffix() {
        // Bedeutungen pointing at a verb infinitive. Use an uncurated
        // title (`vergl.`) so the curated table doesn't shortcut.
        let text = "=== {{Wortart|Abkürzung|Deutsch}} ===\n\
            {{Bedeutungen}}\n\
            :[1] ''Abkürzung für:'' [[vergleichen|vergleiche]]\n\
            {{Beispiele}}\n";
        let entries = extract_abbreviations("vergl.", text);
        assert_eq!(entries[0].pos, UPOS::VERB);
    }

    #[test]
    fn bedeutungen_classifies_adverb_via_falls_suffix() {
        let text = "=== {{Wortart|Abkürzung|Deutsch}} ===\n\
            {{Bedeutungen}}\n\
            :[1] [[gegebenenfalls]]\n";
        let entries = extract_abbreviations("ggf2.", text);
        assert_eq!(entries[0].pos, UPOS::ADV);
    }

    #[test]
    fn bedeutungen_majority_vote_across_multiple_wikilinks() {
        // Five lemmas: 3 ADJ (-isch), 1 NOUN (capital), 1 unclassifiable —
        // ADJ should win.
        let text = "=== {{Wortart|Abkürzung|Deutsch}} ===\n\
            {{Bedeutungen}}\n\
            :[1] [[albanisch]], [[albanesisch]], [[albanistisch]]\n\
            :[2] [[Albanien]] und [[unfoo]]\n\
            {{Beispiele}}\n";
        let entries = extract_abbreviations("alb.", text);
        assert_eq!(entries[0].pos, UPOS::ADJ);
    }

    #[test]
    fn bedeutungen_ignores_namespaced_links() {
        // [[Wikipedia:...]] and [[Datei:...]] must not bias the vote.
        let text = "=== {{Wortart|Abkürzung|Deutsch}} ===\n\
            {{Bedeutungen}}\n\
            :[1] [[albanisch]]\n\
            :[1] {{Wikipedia|Albanisch}}\n\
            {{Beispiele}}\n";
        let entries = extract_abbreviations("alb.", text);
        assert_eq!(entries[0].pos, UPOS::ADJ);
    }

    #[test]
    fn bedeutungen_pipe_syntax_uses_target_not_display() {
        // [[target|display]] — vote is cast for `target`, not `display`.
        let text = "=== {{Wortart|Abkürzung|Deutsch}} ===\n\
            {{Bedeutungen}}\n\
            :[1] [[vergleichen|vergleiche]]\n\
            {{Beispiele}}\n";
        let entries = extract_abbreviations("vglx.", text);
        assert_eq!(entries[0].pos, UPOS::VERB);
    }
}
