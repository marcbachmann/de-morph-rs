//! Extract German verb analyses from a Wiktionary page.
//!
//! This module is now a thin shim:
//!   1. Find every `{{Deutsch Verb Übersicht}}` template on the page.
//!   2. Parse it into a [`VerbAttested`] struct (the small set of
//!      forms Wiktionary actually stores).
//!   3. Hand that to [`generate_verb_paradigm`], which expands to the
//!      full synthetic paradigm (~30 cells per verb).
//!   4. Wrap each paradigm cell as an `ExtractedEntry` so downstream
//!      tooling sees the same shape it sees for nouns.
//!
//! The expansion is unconditional: every verb gets the full paradigm.
//! `Source` tags carried through from [`generate_verb_paradigm`]
//! distinguish lexicon-attested forms from rule-derived ones.
//!
//! References (verified):
//! - Template documentation:
//!   <https://de.wiktionary.org/wiki/Vorlage:Deutsch_Verb_%C3%9Cbersicht>
//! - Parameter inventory verified against three real entries
//!   (`lieben`, `sein`, `klieben`) sampled from the 20260601 dump.

use crate::analysis::{Aux, UPOS};
use crate::paradigm::verb::{VerbAttested, generate_verb_paradigm};
use crate::wiktionary::ExtractedEntry;
use crate::wiktionary::template::Template;
use crate::wiktionary::template::find_templates;

/// Extract all verb analyses (full paradigm) from a Wiktionary page.
pub fn extract_verbs(title: &str, page_text: &str) -> Vec<ExtractedEntry> {
    let mut out = Vec::new();
    for tpl in find_templates(page_text) {
        if !is_verb_overview_template(tpl.name) {
            continue;
        }
        let inputs = parse_verb_template(title, &tpl);
        // The perfect-tense auxiliary (haben/sein/both) is a lexical
        // property of the verb, read from the `Hilfsverb` field and
        // stamped on every cell of the paradigm.
        let aux = parse_hilfsverb(tpl.named_arg("Hilfsverb"));
        for (surface, mut analysis) in generate_verb_paradigm(&inputs) {
            analysis.features.aux = aux;
            out.push(ExtractedEntry {
                surface,
                lemma: title.to_string(),
                pos: UPOS::VERB,
                features: analysis.features,
                source: analysis.source,
                source_title: title.to_string(),
            });
        }
    }
    out
}

/// Recognise the verb-overview template family.
fn is_verb_overview_template(name: &str) -> bool {
    let n = name.trim();
    n == "Deutsch Verb Übersicht"
        || n.starts_with("Deutsch Verb Übersicht ")
        || n.starts_with("Deutsch Verb Übersicht-")
}

/// Parse a verb-overview template into a [`VerbAttested`]. Empty cell
/// strings are mapped to `None` so the paradigm generator does not
/// expand from blanks.
pub fn parse_verb_template<'a>(title: &'a str, tpl: &Template<'a>) -> VerbAttested<'a> {
    VerbAttested {
        infinitive: title,
        present_1sg: non_empty(tpl.named_arg("Präsens_ich")),
        present_2sg: non_empty(tpl.named_arg("Präsens_du")),
        present_3sg: non_empty(tpl.named_arg("Präsens_er, sie, es")),
        past_1sg: non_empty(tpl.named_arg("Präteritum_ich")),
        konj_ii_1sg: non_empty(tpl.named_arg("Konjunktiv II_ich")),
        imperativ_sg: non_empty(tpl.named_arg("Imperativ Singular")),
        imperativ_pl: non_empty(tpl.named_arg("Imperativ Plural")),
        partizip_perf: non_empty(tpl.named_arg("Partizip II")),
    }
}

/// Map the Wiktionary `Hilfsverb` field to an [`Aux`]. The field holds
/// `haben`, `sein`, or both (`haben, sein` / `sein und haben`); we detect
/// each keyword's presence so any separator works.
fn parse_hilfsverb(value: Option<&str>) -> Option<Aux> {
    let v = non_empty(value)?;
    let has_haben = v.contains("haben");
    let has_sein = v.contains("sein");
    match (has_haben, has_sein) {
        (true, true) => Some(Aux::Both),
        (true, false) => Some(Aux::Haben),
        (false, true) => Some(Aux::Sein),
        (false, false) => None,
    }
}

#[inline]
fn non_empty(value: Option<&str>) -> Option<&str> {
    value.and_then(|s| {
        let t = s.trim();
        if t.is_empty() || t == "—" {
            None
        } else {
            Some(t)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{Aux, Mood, Number, Person, Source, Tense, VerbForm};

    fn page(body: &str) -> String {
        format!("== Headword ({{{{Sprache|Deutsch}}}}) ==\n{{{{{body}}}}}\n")
    }

    #[test]
    fn hilfsverb_maps_to_aux() {
        assert_eq!(parse_hilfsverb(Some("haben")), Some(Aux::Haben));
        assert_eq!(parse_hilfsverb(Some("sein")), Some(Aux::Sein));
        assert_eq!(parse_hilfsverb(Some("haben, sein")), Some(Aux::Both));
        assert_eq!(parse_hilfsverb(Some("sein und haben")), Some(Aux::Both));
        assert_eq!(parse_hilfsverb(Some("—")), None);
        assert_eq!(parse_hilfsverb(None), None);
    }

    #[test]
    fn lieben_expands_to_full_paradigm() {
        let body = "Deutsch Verb Übersicht\n\
            |Präsens_ich=liebe\n\
            |Präsens_du=liebst\n\
            |Präsens_er, sie, es=liebt\n\
            |Präteritum_ich=liebte\n\
            |Partizip II=geliebt\n\
            |Konjunktiv II_ich=liebte\n\
            |Imperativ Singular=liebe\n\
            |Imperativ Plural=liebt\n\
            |Hilfsverb=haben";
        let entries = extract_verbs("lieben", &page(body));

        // The paradigm generator produces 30 cells per verb (see
        // paradigm::verb::tests::lieben_full_paradigm_size).
        assert_eq!(entries.len(), 30, "expected 30, got {entries:#?}");

        for e in &entries {
            assert_eq!(e.lemma, "lieben");
            assert_eq!(e.source_title, "lieben");
            assert_eq!(e.pos, UPOS::VERB);
            // The Hilfsverb=haben field is stamped on every cell.
            assert_eq!(e.features.aux, Some(Aux::Haben));
        }

        // Spot-check that attested forms are tagged Lexicon and derived
        // forms tagged Generated.
        let attested_1sg = entries
            .iter()
            .find(|e| {
                e.features.person == Some(Person::P1)
                    && e.features.number == Some(Number::Sg)
                    && e.features.tense == Some(Tense::Pres)
                    && e.features.mood == Some(Mood::Ind)
                    && e.features.form == Some(VerbForm::Fin)
            })
            .unwrap();
        assert_eq!(attested_1sg.surface, "liebe");
        assert_eq!(attested_1sg.source, Source::Lexicon);

        let derived_2pl = entries
            .iter()
            .find(|e| {
                e.features.person == Some(Person::P2)
                    && e.features.number == Some(Number::Pl)
                    && e.features.tense == Some(Tense::Pres)
                    && e.features.mood == Some(Mood::Ind)
                    && e.features.form == Some(VerbForm::Fin)
            })
            .unwrap();
        assert_eq!(derived_2pl.surface, "liebt");
        assert_eq!(derived_2pl.source, Source::Generated);
    }

    #[test]
    fn parse_template_drops_empty_cells() {
        // Wiktionary occasionally leaves a parameter blank; that
        // shouldn't propagate as `Some("")` into the paradigm
        // generator (it would emit empty surfaces).
        let body = "Deutsch Verb Übersicht\n\
            |Präsens_ich=liebe\n\
            |Präsens_du=\n\
            |Partizip II=—";
        let text = page(body);
        let templates = find_templates(&text);
        let verb_tpl = templates
            .iter()
            .find(|t| is_verb_overview_template(t.name))
            .expect("verb template not found in test page");
        let inputs = parse_verb_template("lieben", verb_tpl);
        assert_eq!(inputs.present_1sg, Some("liebe"));
        assert_eq!(inputs.present_2sg, None);
        assert_eq!(inputs.partizip_perf, None);
    }

    #[test]
    fn non_verb_template_is_ignored() {
        let body = "English Verb Übersicht|Präsens_ich=love";
        let entries = extract_verbs("love", &page(body));
        assert!(entries.is_empty());
    }
}
