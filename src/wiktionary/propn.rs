//! Extract German proper nouns from a Wiktionary page.
//!
//! Wiktionary tags proper nouns with several different Wortart values:
//!   - `Toponym`        — place names (Berlin, München, Zürich): ~11.6k
//!   - `Nachname`       — surnames (Müller, Schmidt, Meier):     ~3.0k
//!   - `Vorname`        — given names (Maria, Hans, Petra):      ~1.7k
//!   - `Eigenname`      — general proper names:                    ~790
//!   - `Göttername`     — deity names (Zeus, Wotan):               ~160
//!   - `Bauwerksname`   — building names (Brandenburger Tor):      ~110
//!   - `Ortsnamengrundwort` — toponym-forming elements:             ~95
//!
//! ~17,500 entries total. They were largely invisible to the previous
//! noun extractor because, although these pages carry a
//! `{{Wortart|Substantiv|Deutsch}}` heading alongside the proper-noun
//! tag, the inflection is in specialised templates
//! (`{{Deutsch Toponym Übersicht}}`, `{{Deutsch Vorname Übersicht}}`,
//! `{{Deutsch Nachname Übersicht}}`) rather than
//! `{{Deutsch Substantiv Übersicht}}`, so the noun extractor's match
//! never fired.
//!
//! Output: one entry per page with `UPOS::PROPN`. For places / surnames /
//! buildings we emit four case cells (Nom/Gen/Dat/Acc Sg) where:
//!   - Nom/Acc/Dat Sg = lemma (German proper nouns are usually uninflected)
//!   - Gen Sg         = lemma + "s" (the productive Fugen-s and Gen-s for
//!                      most proper nouns: Berlins, Müllers, Zürichs)
//! This is the minimum needed to make compound splitting unlock
//! (`Zürichsee` = Zürich + See needs `Zürich` reachable as compound-left).
//! For given names parsed from `Deutsch Vorname Übersicht`, we emit
//! whatever case cells the template carries.
//!
//! References:
//! - Wiktionary Wortart inventory: <https://de.wiktionary.org/wiki/Vorlage:Wortart>
//! - UD PROPN definition: <https://universaldependencies.org/u/pos/PROPN.html>

use crate::analysis::{Case, Features, Gender, Number, Source, UPOS};
use crate::wiktionary::template::{find_templates, Template};
use crate::wiktionary::ExtractedEntry;

/// Wortart values that classify a page as a proper noun (PROPN).
const PROPN_WORTART: &[&str] = &[
    "{{Wortart|Toponym|Deutsch}}",
    "{{Wortart|Nachname|Deutsch}}",
    "{{Wortart|Vorname|Deutsch}}",
    "{{Wortart|Eigenname|Deutsch}}",
    "{{Wortart|Göttername|Deutsch}}",
    "{{Wortart|Bauwerksname|Deutsch}}",
    "{{Wortart|Ortsnamengrundwort|Deutsch}}",
];

/// Extract proper-noun entries from a Wiktionary page.
pub fn extract_proper_nouns(title: &str, page_text: &str) -> Vec<ExtractedEntry> {
    if !has_german_propn_section(page_text) {
        return Vec::new();
    }
    // If a structured Vorname Übersicht is present, parse its case
    // cells. Otherwise fall back to the canonical 4-cell paradigm
    // (Nom/Dat/Acc = lemma, Gen = lemma+s).
    let gender = detect_vorname_gender(page_text);
    if let Some(tpl) = find_templates(page_text)
        .into_iter()
        .find(|t| is_vorname_overview_template(t.name))
    {
        let cells = parse_vorname_cells(&tpl, title, gender);
        if !cells.is_empty() {
            return cells;
        }
    }
    canonical_propn_paradigm(title, gender)
}

fn has_german_propn_section(page_text: &str) -> bool {
    // Cheap reject before the linear scan.
    if !page_text.contains("Wortart") {
        return false;
    }
    PROPN_WORTART.iter().any(|t| page_text.contains(t))
}

fn is_vorname_overview_template(name: &str) -> bool {
    let n = name.trim();
    n == "Deutsch Vorname Übersicht"
        || n == "Deutsch Vorname Übersicht m"
        || n == "Deutsch Vorname Übersicht f"
        || n == "Deutsch Vorname Übersicht n"
}

/// Determine gender from the Wortart-line shorthand `{{m}}`/`{{f}}`/`{{n}}`
/// or from the Vorname Übersicht template suffix.
fn detect_vorname_gender(page_text: &str) -> Option<Gender> {
    if page_text.contains("Deutsch Vorname Übersicht f") {
        return Some(Gender::Fem);
    }
    if page_text.contains("Deutsch Vorname Übersicht m") {
        return Some(Gender::Masc);
    }
    if page_text.contains("Deutsch Vorname Übersicht n") {
        return Some(Gender::Neut);
    }
    // Fallback: scan the wortart heading.
    if page_text.contains(", {{f}}") {
        return Some(Gender::Fem);
    }
    if page_text.contains(", {{m}}") {
        return Some(Gender::Masc);
    }
    if page_text.contains(", {{n}}") {
        return Some(Gender::Neut);
    }
    None
}

/// Parse the Nominativ/Genitiv/Dativ/Akkusativ Singular/Plural cells
/// out of a `{{Deutsch Vorname Übersicht}}` template into individual
/// proper-noun entries (one per (case, number) cell).
fn parse_vorname_cells(
    tpl: &Template<'_>,
    title: &str,
    gender: Option<Gender>,
) -> Vec<ExtractedEntry> {
    let mut out = Vec::new();
    let cases = [
        ("Nominativ", Case::Nom),
        ("Genitiv", Case::Gen),
        ("Dativ", Case::Dat),
        ("Akkusativ", Case::Acc),
    ];
    let numbers = [("Singular", Number::Sg), ("Plural", Number::Pl)];
    for (case_de, case) in &cases {
        for (num_de, number) in &numbers {
            let key_main = format!("{case_de} {num_de}");
            let cells = [
                tpl.named_arg(&key_main),
                tpl.named_arg(&format!("{key_main}*")),
                tpl.named_arg(&format!("{key_main}**")),
            ];
            for cell in cells.into_iter().flatten() {
                let surface = cell.trim();
                if surface.is_empty() || surface == "—" {
                    continue;
                }
                out.push(ExtractedEntry {
                    surface: surface.to_string(),
                    lemma: title.to_string(),
                    pos: UPOS::PROPN,
                    features: Features {
                        case: Some(*case),
                        number: Some(*number),
                        gender,
                        ..Features::empty()
                    },
                    source: Source::Attested,
                    source_title: title.to_string(),
                });
            }
        }
    }
    out
}

/// Canonical 4-cell proper-noun paradigm for places, surnames, building
/// names, etc.: Nom/Dat/Acc Sg = lemma, Gen Sg = lemma + "s". Returning
/// the genitive form unlocks compound splitting on `Zürichsee` →
/// `Zürich` + `See` (Gen-s linker).
fn canonical_propn_paradigm(title: &str, gender: Option<Gender>) -> Vec<ExtractedEntry> {
    let mut out = Vec::with_capacity(4);
    let make = |surface: String, case: Case| ExtractedEntry {
        surface,
        lemma: title.to_string(),
        pos: UPOS::PROPN,
        features: Features {
            case: Some(case),
            number: Some(Number::Sg),
            gender,
            ..Features::empty()
        },
        source: Source::Attested,
        source_title: title.to_string(),
    };
    out.push(make(title.to_string(), Case::Nom));
    out.push(make(title.to_string(), Case::Dat));
    out.push(make(title.to_string(), Case::Acc));
    // German Gen-s of proper nouns. We avoid double-s when the lemma
    // already ends in s/ß/x/z (those typically take an apostrophe or
    // no overt marker — Marx' rather than Marxs).
    let last = title.chars().last();
    let needs_s = !matches!(last, Some('s' | 'ß' | 'x' | 'z' | 'S' | 'X' | 'Z'));
    if needs_s {
        let mut genitive = title.to_string();
        genitive.push('s');
        out.push(make(genitive, Case::Gen));
    } else {
        out.push(make(title.to_string(), Case::Gen));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toponym_emits_canonical_paradigm() {
        let text = "== Berlin ({{Sprache|Deutsch}}) ==\n\
            === {{Wortart|Substantiv|Deutsch}}, {{n}}, {{Wortart|Toponym|Deutsch}} ===";
        let entries = extract_proper_nouns("Berlin", text);
        // Nom/Dat/Acc = "Berlin", Gen = "Berlins" — 4 cells.
        assert_eq!(entries.len(), 4);
        assert!(entries.iter().all(|e| e.pos == UPOS::PROPN));
        assert!(entries.iter().all(|e| e.lemma == "Berlin"));
        let genitive = entries
            .iter()
            .find(|e| e.features.case == Some(Case::Gen))
            .unwrap();
        assert_eq!(genitive.surface, "Berlins");
        assert_eq!(genitive.features.gender, Some(Gender::Neut));
    }

    #[test]
    fn surname_emits_canonical_paradigm() {
        let text = "=== {{Wortart|Substantiv|Deutsch}}, {{m}}, {{Wortart|Nachname|Deutsch}} ===";
        let entries = extract_proper_nouns("Müller", text);
        assert_eq!(entries.len(), 4);
        let genitive = entries
            .iter()
            .find(|e| e.features.case == Some(Case::Gen))
            .unwrap();
        assert_eq!(genitive.surface, "Müllers");
    }

    #[test]
    fn vorname_with_structured_template_emits_cells() {
        let text = "== Maria ({{Sprache|Deutsch}}) ==\n\
            === {{Wortart|Substantiv|Deutsch}}, {{f}}, {{Wortart|Vorname|Deutsch}} ===\n\
            {{Deutsch Vorname Übersicht f\n\
            |Nominativ Singular=Maria\n\
            |Nominativ Plural=Marias\n\
            |Genitiv Singular=Marias\n\
            |Genitiv Singular*=Mariens\n\
            |Dativ Singular=Maria\n\
            |Akkusativ Singular=Maria\n\
            }}";
        let entries = extract_proper_nouns("Maria", text);
        // At least 6 cells (Nom Sg, Nom Pl, Gen Sg, Gen Sg*, Dat Sg, Akk Sg).
        assert!(entries.len() >= 6, "got {} entries", entries.len());
        assert!(entries.iter().all(|e| e.pos == UPOS::PROPN));
        assert!(entries
            .iter()
            .all(|e| e.features.gender == Some(Gender::Fem)));
        assert!(entries.iter().any(|e| e.surface == "Mariens"));
    }

    #[test]
    fn lemma_ending_in_s_keeps_gen_without_double_s() {
        let text = "=== {{Wortart|Substantiv|Deutsch}}, {{m}}, {{Wortart|Nachname|Deutsch}} ===";
        let entries = extract_proper_nouns("Marx", text);
        let genitive = entries
            .iter()
            .find(|e| e.features.case == Some(Case::Gen))
            .unwrap();
        assert_eq!(genitive.surface, "Marx");
    }

    #[test]
    fn page_without_propn_wortart_yields_nothing() {
        let text = "=== {{Wortart|Substantiv|Deutsch}} ===";
        let entries = extract_proper_nouns("Tisch", text);
        assert!(entries.is_empty());
    }

    #[test]
    fn eigenname_wortart_recognised() {
        let text = "=== {{Wortart|Eigenname|Deutsch}} ===";
        let entries = extract_proper_nouns("Apollo", text);
        assert!(!entries.is_empty());
        assert!(entries.iter().all(|e| e.pos == UPOS::PROPN));
    }
}
