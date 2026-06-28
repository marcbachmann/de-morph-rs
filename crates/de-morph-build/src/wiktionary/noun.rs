//! Extract German noun analyses from a Wiktionary page.
//!
//! The strategy is single-pass: locate `{{Deutsch Substantiv Übersicht}}`
//! (or a closely-named variant) in the page wikitext, parse it, and emit
//! one [`ExtractedEntry`] per (surface form, case, number) cell that is
//! present in the template. Multiple analyses of the same surface form are
//! NOT collapsed — Tisch is Nom Sg AND Akk Sg AND Dat Sg, and the FST
//! build step downstream needs all three.
//!
//! References (verified):
//! - Template documentation:
//!   <https://de.wiktionary.org/wiki/Vorlage:Deutsch_Substantiv_%C3%9Cbersicht>
//! - Genus codes (m/f/n): same page.
//! - The template-parameter names parsed below are matters of fact about
//!   the Wiktionary template and are uncopyrightable.

use std::borrow::Cow;

use de_morph::analysis::{Case, Features, Gender, Number, Source, UPOS};

use crate::wiktionary::template::{find_templates, Template};
use crate::wiktionary::ExtractedEntry;

/// Extract all noun analyses from a Wiktionary page.
///
/// Returns the entries in the order they appear in the template; no
/// deduplication, no surface-form collapsing.
pub fn extract_nouns(title: &str, page_text: &str) -> Vec<ExtractedEntry> {
    let mut out = Vec::new();
    for tpl in find_templates(page_text) {
        if !is_noun_overview_template(tpl.name) {
            continue;
        }
        extract_one_template(title, &tpl, &mut out);
    }
    out
}

/// Recognise the noun-overview template family.
///
/// Wiktionary uses several closely-named variants for nominal categories:
///   - `Deutsch Substantiv Übersicht`            — common nouns
///   - `Deutsch Substantiv Übersicht -sch`       — `-sch` suffix subtype
///   - `Deutsch Substantiv Übersicht - schwach`  — weak declension noun
///   - `Deutsch Substantiv Übersicht - regelmäßig` — regular paradigm
///   - `Deutsch Nachname Übersicht`              — surnames
///   - `Deutsch Vorname Übersicht m`             — masc. given names
///   - `Deutsch Toponym Übersicht`               — place names
///   - `Deutsch adjektivische Deklination`       — substantivised adjectives
///
/// We accept the common-noun overview and its decorated variants here.
/// Given-name / surname / toponym templates produce mostly the same
/// fields but have additional quirks (no plural for given names etc.)
/// and are out of scope for this matcher.
fn is_noun_overview_template(name: &str) -> bool {
    let n = name.trim();
    n == "Deutsch Substantiv Übersicht"
        || n.starts_with("Deutsch Substantiv Übersicht ")
        || n.starts_with("Deutsch Substantiv Übersicht-")
}

/// Cell coordinates in the case × number grid.
const CASES: [(Case, &str); 4] = [
    (Case::Nom, "Nominativ"),
    (Case::Gen, "Genitiv"),
    (Case::Dat, "Dativ"),
    (Case::Acc, "Akkusativ"),
];
const NUMBERS: [(Number, &str); 2] = [(Number::Sg, "Singular"), (Number::Pl, "Plural")];

/// Maximum numbered-variant suffix (`Nominativ Singular 1`, `... 2`, ...).
/// Wiktionary occasionally uses up to 4 variants for words with multiple
/// inflection paradigms; the loop stops as soon as a numbered slot is
/// missing.
const MAX_NUMBERED_VARIANT: usize = 4;

fn extract_one_template(title: &str, tpl: &Template, out: &mut Vec<ExtractedEntry>) {
    // Wiktionary supports up to two parallel genders per entry
    // (e.g. "der/das Joghurt"). The first is in `Genus` or `Genus 1`,
    // the second in `Genus 2`.
    let genders: Vec<Gender> = ["Genus", "Genus 1", "Genus 2"]
        .iter()
        .filter_map(|k| tpl.named_arg(k).and_then(parse_gender))
        .collect();
    if genders.is_empty() {
        // Some lemmas (e.g. pluraliatantum) may legitimately omit Genus.
        // We skip them rather than guessing a gender.
        return;
    }

    let lemma = lemma_form(title, tpl);

    for &gender in &genders {
        for &(number, number_name) in &NUMBERS {
            for &(case, case_name) in &CASES {
                // Plain cell: e.g. "Nominativ Singular".
                let plain_key = format!("{case_name} {number_name}");
                push_if_present(
                    out,
                    title,
                    &lemma,
                    gender,
                    number,
                    case,
                    tpl.named_arg(&plain_key),
                );

                // Alternative form (asterisk): e.g. "Genitiv Singular*".
                let star_key = format!("{case_name} {number_name}*");
                push_if_present(
                    out,
                    title,
                    &lemma,
                    gender,
                    number,
                    case,
                    tpl.named_arg(&star_key),
                );

                // Numbered variants: "Nominativ Singular 1", "Nominativ
                // Singular 2", etc. Stop at the first missing slot.
                for n in 1..=MAX_NUMBERED_VARIANT {
                    let numbered_key = format!("{case_name} {number_name} {n}");
                    match tpl.named_arg(&numbered_key) {
                        Some(form) if !form.is_empty() => {
                            push_if_present(out, title, &lemma, gender, number, case, Some(form));
                        }
                        _ => break,
                    }
                }
            }
        }
    }
}

#[inline]
fn push_if_present(
    out: &mut Vec<ExtractedEntry>,
    title: &str,
    lemma: &str,
    gender: Gender,
    number: Number,
    case: Case,
    form: Option<&str>,
) {
    let raw = match form {
        Some(s) => s,
        None => return,
    };
    let clean = strip_wikitext_noise(raw);
    // Skip dash placeholders ("—"/"-") that Wiktionary uses for missing
    // number cells (singulare-/pluraletantum) — they are not word forms.
    if !super::is_real_form(&clean) {
        return;
    }
    out.push(ExtractedEntry {
        surface: clean.into_owned(),
        lemma: lemma.to_string(),
        pos: UPOS::NOUN,
        features: Features::noun_form(gender, number, case),
        source: Source::Attested,
        source_title: title.to_string(),
    });
}

/// Strip a small set of wikitext artefacts that occasionally appear in
/// noun-overview cells:
///   - `<br />` / `<br/>` between two alternative forms
///   - `&nbsp;` non-breaking spaces
///   - leading/trailing whitespace
///
/// Anything more elaborate (HTML comments, `<ref>...</ref>` citations) is
/// left for the next pass — we'd rather skip a row than emit corrupted
/// data.
fn strip_wikitext_noise(s: &str) -> Cow<'_, str> {
    let trimmed = s.trim();
    if !trimmed.contains('<') && !trimmed.contains('&') {
        return Cow::Borrowed(trimmed);
    }
    let mut buf = String::with_capacity(trimmed.len());
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip <br/> / <br /> tags.
        if bytes[i] == b'<' {
            let rest = &trimmed[i..];
            if rest.starts_with("<br") {
                if let Some(close) = rest.find('>') {
                    i += close + 1;
                    buf.push(' ');
                    continue;
                }
            }
            // Skip any other tag we don't know about by erring on the
            // side of dropping the row — caller will reject empties.
            return Cow::Owned(String::new());
        }
        // Recognise common HTML entities.
        if bytes[i] == b'&' {
            let rest = &trimmed[i..];
            if let Some(semi) = rest.find(';') {
                let entity = &rest[..=semi];
                match entity {
                    "&nbsp;" => buf.push(' '),
                    "&amp;" => buf.push('&'),
                    "&lt;" => buf.push('<'),
                    "&gt;" => buf.push('>'),
                    _ => return Cow::Owned(String::new()),
                }
                i += entity.len();
                continue;
            }
        }
        // Decode multibyte UTF-8 in one step so we don't split codepoints.
        let ch = trimmed[i..].chars().next().unwrap();
        buf.push(ch);
        i += ch.len_utf8();
    }
    Cow::Owned(buf.trim().to_string())
}

/// The lemma form for an extracted noun entry.
///
/// Default: the Wiktionary page title (this is the canonical Wiktionary
/// convention — the page name IS the lemma). If the template marks
/// "kein Singular = 1", the lemma is still the page title (Wiktionary
/// uses the Nominativ Plural as the page title for Pluraliatantum).
///
/// We accept the page title verbatim because it is also the lemma the
/// Wiktionary attribution chain points to (the article URL).
fn lemma_form(title: &str, _tpl: &Template) -> String {
    title.to_string()
}

fn parse_gender(s: &str) -> Option<Gender> {
    match s.trim() {
        "m" | "M" => Some(Gender::Masc),
        "f" | "F" => Some(Gender::Fem),
        "n" | "N" => Some(Gender::Neut),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a test page body for one noun overview template.
    fn page(body: &str) -> String {
        format!(
            "== Headword ({{{{Sprache|Deutsch}}}}) ==\n\
             === {{{{Wortart|Substantiv|Deutsch}}}}, {{{{m}}}} ===\n\
             {{{{{body}}}}}\n"
        )
    }

    #[test]
    fn tisch_full_paradigm() {
        // Adapted from <https://de.wiktionary.org/wiki/Tisch> (template
        // body only; uncopyrightable factual content).
        let body = "Deutsch Substantiv Übersicht\n\
            |Genus=m\n\
            |Nominativ Singular=Tisch\n\
            |Genitiv Singular=Tisches\n\
            |Dativ Singular=Tisch\n\
            |Akkusativ Singular=Tisch\n\
            |Nominativ Plural=Tische\n\
            |Genitiv Plural=Tische\n\
            |Dativ Plural=Tischen\n\
            |Akkusativ Plural=Tische";
        let entries = extract_nouns("Tisch", &page(body));
        assert_eq!(entries.len(), 8, "8 cells expected, got {entries:#?}");

        // All entries have masculine gender and the right lemma.
        for e in &entries {
            assert_eq!(e.pos, UPOS::NOUN);
            assert_eq!(e.features.gender, Some(Gender::Masc));
            assert_eq!(e.lemma, "Tisch");
            assert_eq!(e.source_title, "Tisch");
        }

        // Spot-check a few cells.
        assert_eq!(entries[0].surface, "Tisch");
        assert_eq!(entries[0].features.case, Some(Case::Nom));
        assert_eq!(entries[0].features.number, Some(Number::Sg));

        let dat_pl = entries
            .iter()
            .find(|e| e.features.case == Some(Case::Dat) && e.features.number == Some(Number::Pl))
            .unwrap();
        assert_eq!(dat_pl.surface, "Tischen");
    }

    #[test]
    fn alternative_genitive_singular_emits_extra_entry() {
        // Tisch has a documented variant `Tischs` (alongside `Tisches`).
        let body = "Deutsch Substantiv Übersicht\n\
            |Genus=m\n\
            |Nominativ Singular=Tisch\n\
            |Genitiv Singular=Tisches\n\
            |Genitiv Singular*=Tischs\n\
            |Dativ Singular=Tisch\n\
            |Akkusativ Singular=Tisch\n\
            |Nominativ Plural=Tische\n\
            |Genitiv Plural=Tische\n\
            |Dativ Plural=Tischen\n\
            |Akkusativ Plural=Tische";
        let entries = extract_nouns("Tisch", &page(body));
        let gen_sg: Vec<_> = entries
            .iter()
            .filter(|e| e.features.case == Some(Case::Gen) && e.features.number == Some(Number::Sg))
            .collect();
        assert_eq!(gen_sg.len(), 2);
        let surfaces: Vec<&str> = gen_sg.iter().map(|e| e.surface.as_str()).collect();
        assert!(surfaces.contains(&"Tisches"));
        assert!(surfaces.contains(&"Tischs"));
    }

    #[test]
    fn missing_singular_yields_plural_only() {
        // Pluraliatantum: "Eltern".
        let body = "Deutsch Substantiv Übersicht\n\
            |Genus=f\n\
            |kein Singular=1\n\
            |Nominativ Plural=Eltern\n\
            |Genitiv Plural=Eltern\n\
            |Dativ Plural=Eltern\n\
            |Akkusativ Plural=Eltern";
        let entries = extract_nouns("Eltern", &page(body));
        assert_eq!(entries.len(), 4);
        assert!(entries
            .iter()
            .all(|e| e.features.number == Some(Number::Pl)));
    }

    #[test]
    fn missing_plural_yields_singular_only() {
        // Singulariatantum: "Milch".
        let body = "Deutsch Substantiv Übersicht\n\
            |Genus=f\n\
            |Nominativ Singular=Milch\n\
            |Genitiv Singular=Milch\n\
            |Dativ Singular=Milch\n\
            |Akkusativ Singular=Milch\n\
            |kein Plural=1";
        let entries = extract_nouns("Milch", &page(body));
        assert_eq!(entries.len(), 4);
        assert!(entries
            .iter()
            .all(|e| e.features.number == Some(Number::Sg)));
    }

    #[test]
    fn umlaut_plural_is_preserved() {
        // Mann → Männer.
        let body = "Deutsch Substantiv Übersicht\n\
            |Genus=m\n\
            |Nominativ Singular=Mann\n\
            |Genitiv Singular=Mannes\n\
            |Dativ Singular=Mann\n\
            |Akkusativ Singular=Mann\n\
            |Nominativ Plural=Männer\n\
            |Genitiv Plural=Männer\n\
            |Dativ Plural=Männern\n\
            |Akkusativ Plural=Männer";
        let entries = extract_nouns("Mann", &page(body));
        let nom_pl = entries
            .iter()
            .find(|e| e.features.case == Some(Case::Nom) && e.features.number == Some(Number::Pl))
            .unwrap();
        assert_eq!(nom_pl.surface, "Männer");
        let dat_pl = entries
            .iter()
            .find(|e| e.features.case == Some(Case::Dat) && e.features.number == Some(Number::Pl))
            .unwrap();
        assert_eq!(dat_pl.surface, "Männern");
    }

    #[test]
    fn dual_gender_emits_both() {
        // "Joghurt" — der/die/das Joghurt (we take the first two).
        let body = "Deutsch Substantiv Übersicht\n\
            |Genus 1=m\n\
            |Genus 2=n\n\
            |Nominativ Singular=Joghurt\n\
            |Genitiv Singular=Joghurts\n\
            |Dativ Singular=Joghurt\n\
            |Akkusativ Singular=Joghurt\n\
            |Nominativ Plural=Joghurts\n\
            |Genitiv Plural=Joghurts\n\
            |Dativ Plural=Joghurts\n\
            |Akkusativ Plural=Joghurts";
        let entries = extract_nouns("Joghurt", &page(body));
        // 2 genders × 4 cases × 2 numbers = 16 entries.
        assert_eq!(entries.len(), 16);
        let m = entries
            .iter()
            .filter(|e| e.features.gender == Some(Gender::Masc))
            .count();
        let n = entries
            .iter()
            .filter(|e| e.features.gender == Some(Gender::Neut))
            .count();
        assert_eq!(m, 8);
        assert_eq!(n, 8);
    }

    #[test]
    fn numbered_variants_emitted() {
        // Wort → Worte/Wörter (two parallel plural paradigms).
        let body = "Deutsch Substantiv Übersicht\n\
            |Genus=n\n\
            |Nominativ Singular=Wort\n\
            |Genitiv Singular=Wortes\n\
            |Dativ Singular=Wort\n\
            |Akkusativ Singular=Wort\n\
            |Nominativ Plural 1=Worte\n\
            |Nominativ Plural 2=Wörter\n\
            |Genitiv Plural 1=Worte\n\
            |Genitiv Plural 2=Wörter\n\
            |Dativ Plural 1=Worten\n\
            |Dativ Plural 2=Wörtern\n\
            |Akkusativ Plural 1=Worte\n\
            |Akkusativ Plural 2=Wörter";
        let entries = extract_nouns("Wort", &page(body));
        let nom_pl: Vec<&str> = entries
            .iter()
            .filter(|e| e.features.case == Some(Case::Nom) && e.features.number == Some(Number::Pl))
            .map(|e| e.surface.as_str())
            .collect();
        assert!(nom_pl.contains(&"Worte"));
        assert!(nom_pl.contains(&"Wörter"));
    }

    #[test]
    fn empty_cell_is_dropped() {
        // Some templates leave a cell empty rather than omitting it.
        let body = "Deutsch Substantiv Übersicht\n\
            |Genus=m\n\
            |Nominativ Singular=Test\n\
            |Genitiv Singular=\n\
            |Dativ Singular=Test\n\
            |Akkusativ Singular=Test";
        let entries = extract_nouns("Test", &page(body));
        assert!(entries
            .iter()
            .all(|e| !(e.features.case == Some(Case::Gen) && e.surface.is_empty())));
        assert!(!entries
            .iter()
            .any(|e| e.features.case == Some(Case::Gen) && e.features.number == Some(Number::Sg)));
    }

    #[test]
    fn em_dash_cell_is_dropped() {
        // Some templates put an em dash to indicate "form does not exist".
        let body = "Deutsch Substantiv Übersicht\n\
            |Genus=m\n\
            |Nominativ Singular=Test\n\
            |Genitiv Singular=—\n\
            |Dativ Singular=Test\n\
            |Akkusativ Singular=Test";
        let entries = extract_nouns("Test", &page(body));
        assert!(!entries
            .iter()
            .any(|e| e.features.case == Some(Case::Gen) && e.features.number == Some(Number::Sg)));
    }

    #[test]
    fn nbsp_in_form_is_normalised() {
        let body = "Deutsch Substantiv Übersicht\n\
            |Genus=m\n\
            |Nominativ Singular=ABC&nbsp;DEF\n\
            |Genitiv Singular=ABC&nbsp;DEFs\n\
            |Dativ Singular=ABC DEF\n\
            |Akkusativ Singular=ABC DEF";
        let entries = extract_nouns("ABC DEF", &page(body));
        let nom = entries
            .iter()
            .find(|e| e.features.case == Some(Case::Nom))
            .unwrap();
        assert_eq!(nom.surface, "ABC DEF");
    }

    #[test]
    fn non_german_template_is_ignored() {
        let body = "English Substantiv Übersicht|Genus=m|Nominativ Singular=Tisch";
        let entries = extract_nouns("Tisch", &page(body));
        assert!(entries.is_empty());
    }

    #[test]
    fn template_without_genus_is_skipped() {
        // No Genus key — extractor must not produce entries.
        let body = "Deutsch Substantiv Übersicht\n\
            |Nominativ Singular=X\n\
            |Akkusativ Singular=X";
        let entries = extract_nouns("X", &page(body));
        assert!(entries.is_empty());
    }
}
