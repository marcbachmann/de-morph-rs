//! Extract German adjective analyses from a Wiktionary page.
//!
//! Like the verb extractor, this module is a thin shim: parse the
//! `{{Deutsch Adjektiv Übersicht}}` template into an [`AdjectiveAttested`]
//! and hand off to [`generate_adjective_paradigm`].
//!
//! References (verified):
//! - Template documentation:
//!   <https://de.wiktionary.org/wiki/Vorlage:Deutsch_Adjektiv_%C3%9Cbersicht>
//! - Parameter inventory confirmed against four real entries
//!   (`pittoresk`, `infinitesimal`, `lieb`, `rot`) sampled from the
//!   20260601 dump.

use de_morph::analysis::UPOS;
use de_morph::paradigm::adjective::{generate_adjective_paradigm, AdjectiveAttested};
use crate::wiktionary::template::{find_templates, Template};
use crate::wiktionary::ExtractedEntry;

/// Extract all adjective analyses from a Wiktionary page.
///
/// When the template carries the explicit `keine weiteren Formen=ja`
/// flag (used for `prima`, `super`, `klasse`, `extra` and similar
/// strictly-indeclinable adjectives), we emit ONLY the bare lemma
/// rather than the 73-cell paradigm — those adjectives have no
/// inflected forms in any register, neither Standardsprache nor
/// Umgangssprache.
///
/// We deliberately do NOT suppress paradigm generation for adjectives
/// like `rosa` / `lila` / `beige`, even though the same `Komparativ=—`
/// pattern applies. Those are indeklinabel in Standardsprache but
/// Wiktionary's own prose annotation documents the colloquial forms
/// (rosaner, lilaner, beiger) — those forms are attested in real
/// German text and our analyzer should still recognize them.
pub fn extract_adjectives(title: &str, page_text: &str) -> Vec<ExtractedEntry> {
    let mut out = Vec::new();
    for tpl in find_templates(page_text) {
        if !is_adjective_overview_template(tpl.name) {
            continue;
        }
        if is_strictly_indeclinable(&tpl) {
            out.push(strictly_indeclinable_entry(title));
            continue;
        }
        for inputs in collect_adjective_inputs(title, &tpl) {
            for (surface, analysis) in generate_adjective_paradigm(&inputs) {
                out.push(ExtractedEntry {
                    surface,
                    lemma: title.to_string(),
                    pos: UPOS::ADJ,
                    features: analysis.features,
                    source: analysis.source,
                    source_title: title.to_string(),
                });
            }
        }
    }
    out
}

/// Detect Wiktionary's `keine weiteren Formen=ja` flag, marking an
/// adjective as having no inflected forms at all (predicative-only,
/// strict indeclinability). Wiktionary editors sometimes write the
/// value as `1` or `Ja` rather than `ja`; we accept any non-empty
/// non-"—" value since the flag's mere presence is the signal.
fn is_strictly_indeclinable(tpl: &Template<'_>) -> bool {
    matches!(
        tpl.named_arg("keine weiteren Formen"),
        Some(v) if non_empty(Some(v)).is_some()
    )
}

/// Produce the single entry we emit for a strictly-indeclinable
/// adjective: the bare lemma, predicative-only positive degree, no
/// case/number/gender feature.
fn strictly_indeclinable_entry(title: &str) -> ExtractedEntry {
    use de_morph::analysis::{Degree, Features, Source};
    ExtractedEntry {
        surface: title.to_string(),
        lemma: title.to_string(),
        pos: UPOS::ADJ,
        features: Features {
            degree: Some(Degree::Pos),
            ..Features::empty()
        },
        source: Source::Attested,
        source_title: title.to_string(),
    }
}

fn is_adjective_overview_template(name: &str) -> bool {
    let n = name.trim();
    n == "Deutsch Adjektiv Übersicht"
        || n.starts_with("Deutsch Adjektiv Übersicht ")
        || n.starts_with("Deutsch Adjektiv Übersicht-")
}

/// Build one or more [`AdjectiveAttested`] from a template, expanding
/// the asterisk-variant fields (`Komparativ*`, `Superlativ*`) into
/// separate input bundles so each surface variant gets its own
/// paradigm.
fn collect_adjective_inputs<'a>(title: &'a str, tpl: &Template<'a>) -> Vec<AdjectiveAttested<'a>> {
    let positiv = non_empty(tpl.named_arg("Positiv")).unwrap_or(title);
    let komparativ_main = non_empty(tpl.named_arg("Komparativ"));
    let komparativ_star = non_empty(tpl.named_arg("Komparativ*"));
    let superlativ_main = non_empty(tpl.named_arg("Superlativ"));
    let superlativ_star = non_empty(tpl.named_arg("Superlativ*"));

    let komparatives: Vec<Option<&str>> = once_or_pair(komparativ_main, komparativ_star);
    let superlatives: Vec<Option<&str>> = once_or_pair(superlativ_main, superlativ_star);

    let mut out = Vec::new();
    for &k in &komparatives {
        for &s in &superlatives {
            out.push(AdjectiveAttested {
                lemma: positiv,
                komparativ: k,
                superlativ: s,
            });
        }
    }
    out
}

#[inline]
fn once_or_pair<'a>(a: Option<&'a str>, b: Option<&'a str>) -> Vec<Option<&'a str>> {
    match (a, b) {
        (None, None) => vec![None],
        (Some(_), None) => vec![a],
        (None, Some(_)) => vec![b],
        (Some(_), Some(_)) => vec![a, b],
    }
}

#[inline]
fn non_empty(value: Option<&str>) -> Option<&str> {
    value.and_then(|s| {
        let t = s.trim();
        // Reject dash placeholders ("—"/"-") and suffix abbreviations
        // ("-ibler"): a "no comparative" cell must not seed a paradigm.
        super::is_real_form(t).then_some(t)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use de_morph::analysis::{Case, Declension, Degree, Gender, Number, Source};

    fn page(body: &str) -> String {
        format!("== Headword ({{{{Sprache|Deutsch}}}}) ==\n{{{{{body}}}}}\n")
    }

    #[test]
    fn lieb_full_paradigm() {
        let body = "Deutsch Adjektiv Übersicht\n\
            |Positiv=lieb\n\
            |Komparativ=lieber\n\
            |Superlativ=liebsten";
        let entries = extract_adjectives("lieb", &page(body));
        // 3 predicative + 3 × 72 = 219 cells, all with lemma="lieb".
        assert_eq!(entries.len(), 219);
        for e in &entries {
            assert_eq!(e.lemma, "lieb");
            assert_eq!(e.pos, UPOS::ADJ);
        }
        // Spot-check a few attributive surfaces.
        let strong_nom_masc_pos = entries
            .iter()
            .find(|e| {
                e.features.degree == Some(Degree::Pos)
                    && e.features.declension == Some(Declension::Strong)
                    && e.features.case == Some(Case::Nom)
                    && e.features.number == Some(Number::Sg)
                    && e.features.gender == Some(Gender::Masc)
            })
            .unwrap();
        assert_eq!(strong_nom_masc_pos.surface, "lieber");
        assert_eq!(strong_nom_masc_pos.source, Source::Inflected);

        let weak_dat_pl_sup = entries
            .iter()
            .find(|e| {
                e.features.degree == Some(Degree::Sup)
                    && e.features.declension == Some(Declension::Weak)
                    && e.features.case == Some(Case::Dat)
                    && e.features.number == Some(Number::Pl)
            })
            .unwrap();
        assert_eq!(weak_dat_pl_sup.surface, "liebsten");
    }

    #[test]
    fn rot_with_starred_variants() {
        let body = "Deutsch Adjektiv Übersicht\n\
            |Positiv=rot\n\
            |Komparativ=röter\n\
            |Komparativ*=roter\n\
            |Superlativ=rötesten\n\
            |Superlativ*=rotesten";
        let entries = extract_adjectives("rot", &page(body));
        // Each of 2 comparatives × 2 superlatives = 4 paradigm
        // expansions. The Positiv paradigm is shared so the predicative
        // "rot" cell is duplicated 4 times in raw extraction; the
        // downstream lexicon builder collapses these via its
        // (surface, lemma, pos, features, source) dedup.
        let cmp_surfaces: Vec<&str> = entries
            .iter()
            .filter(|e| {
                e.features.degree == Some(Degree::Cmp)
                    && e.features.declension == Some(Declension::Strong)
                    && e.features.case == Some(Case::Nom)
                    && e.features.number == Some(Number::Sg)
                    && e.features.gender == Some(Gender::Masc)
            })
            .map(|e| e.surface.as_str())
            .collect();
        assert!(cmp_surfaces.contains(&"röterer"));
        assert!(cmp_surfaces.contains(&"roterer"));
    }

    #[test]
    fn no_comparison_emits_only_positive() {
        let body = "Deutsch Adjektiv Übersicht\n\
            |Positiv=infinitesimal\n\
            |Komparativ=—\n\
            |Superlativ=—";
        let entries = extract_adjectives("infinitesimal", &page(body));
        // 1 predicative + 72 attributive = 73.
        assert_eq!(entries.len(), 73);
        assert!(entries
            .iter()
            .all(|e| e.features.degree == Some(Degree::Pos)));
    }

    #[test]
    fn non_adjective_template_is_ignored() {
        let body = "English Adjektiv Übersicht|Positiv=big";
        let entries = extract_adjectives("big", &page(body));
        assert!(entries.is_empty());
    }

    #[test]
    fn strictly_indeclinable_emits_only_lemma() {
        // `prima` carries `keine weiteren Formen=ja` — no inflected
        // forms in any register.
        let body = "Deutsch Adjektiv Übersicht\n\
            |Positiv=prima\n\
            |Komparativ=—\n\
            |Superlativ=—\n\
            |keine weiteren Formen=ja";
        let entries = extract_adjectives("prima", &page(body));
        assert_eq!(entries.len(), 1, "expected only the bare lemma");
        assert_eq!(entries[0].surface, "prima");
        assert_eq!(entries[0].lemma, "prima");
        assert_eq!(entries[0].features.degree, Some(Degree::Pos));
        // Critically: no attributive cells generated.
        assert!(entries[0].features.case.is_none());
        assert!(entries[0].features.number.is_none());
        assert!(entries[0].features.gender.is_none());
    }

    #[test]
    fn rosa_class_keeps_attributive_paradigm() {
        // `rosa` / `lila` / `beige` are indeklinabel in Standardsprache
        // but have colloquial inflected variants (rosaner, lilaner,
        // beiger). Wiktionary uses `Komparativ=—` but does NOT add
        // `keine weiteren Formen=ja`, so we keep emitting the full
        // attributive paradigm so the analyzer can still recognise the
        // colloquial forms.
        let body = "Deutsch Adjektiv Übersicht\n\
            |Positiv=rosa\n\
            |Komparativ=—\n\
            |Superlativ=—";
        let entries = extract_adjectives("rosa", &page(body));
        assert_eq!(
            entries.len(),
            73,
            "rosa-class should still produce its full attributive paradigm \
             (1 predicative + 72 attributive) so colloquial forms remain reachable"
        );
        // The colloquial inflection uses an -n- linker (rosane/rosaner),
        // not the invalid "rosaer"; the bare "rosa" stays uninflected.
        assert!(entries.iter().any(|e| e.surface == "rosaner"));
        assert!(!entries.iter().any(|e| e.surface == "rosaer"));
    }
}
