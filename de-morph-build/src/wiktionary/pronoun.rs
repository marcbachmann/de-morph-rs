//! Extract German closed-class pronoun / determiner analyses from a
//! Wiktionary page.
//!
//! German Wiktionary documents these classes with two structured flexion
//! templates plus a no-table "invariant" case:
//!
//! - `{{Pronomina-Tabelle}}` — full gender × case × number paradigm, used by
//!   articles (`der`, `ein`), demonstratives (`dieser`, `jener`), and the
//!   determiner-like interrogatives (`welcher`). Named args look like
//!   `Nominativ Singular m`, `Genitiv Plural`, with `*` alternants.
//! - `{{Deutsch Pronomen Übersicht}}` — case × number only (no gender), used
//!   by the genderless pronouns (`wer`, `was`, `jemand`, `niemand`).
//! - no table — true invariants such as `allerlei`, `vielerlei`,
//!   `mancherlei`, tagged only with `{{Wortart|Indefinitpronomen|Deutsch}}`.
//!
//! Personal pronouns and possessives are deliberately NOT handled here: their
//! forms live in parameterless meta-templates (`{{Deutsch Personalpronomen N}}`,
//! `{{Deutsch Possessivpronomen}}`) that are absent from the page wikitext, and
//! the possessor-person feature is not recoverable from any table. Those stay
//! in the hand-curated `src/paradigm/closed_class.rs`.
//!
//! To avoid colliding with that hand-curated set, the extractor skips any
//! lemma already produced by `generate_closed_class_entries()` (passed in as
//! `covered`). It therefore only *adds* coverage — closing the gap for
//! `allerlei` and the rest of the indeclinable `-lei` family, `derjenige`,
//! `derselbe`, `irgendein`, `jeglicher`, and friends.
//!
//! References (verified):
//! - `{{Pronomina-Tabelle}}`:
//!   <https://de.wiktionary.org/wiki/Vorlage:Pronomina-Tabelle>
//! - `{{Deutsch Pronomen Übersicht}}`:
//!   <https://de.wiktionary.org/wiki/Vorlage:Deutsch_Pronomen_Übersicht>
//! - Template-parameter names are matters of fact about the template and are
//!   uncopyrightable.

use std::collections::HashSet;

use de_morph::analysis::{Case, Features, Gender, Number, PronType, Source, UPOS};
use crate::wiktionary::template::{find_templates, Template};
use crate::wiktionary::ExtractedEntry;

const PRONOMINA_TABELLE: &str = "Pronomina-Tabelle";
const PRONOMEN_UEBERSICHT: &str = "Deutsch Pronomen Übersicht";

const CASES: [(Case, &str); 4] = [
    (Case::Nom, "Nominativ"),
    (Case::Gen, "Genitiv"),
    (Case::Dat, "Dativ"),
    (Case::Acc, "Akkusativ"),
];

/// Map a German closed-class `{{Wortart|…|Deutsch}}` value to a base UPOS and
/// PronType. Returns `None` for categories handled elsewhere (personal,
/// possessive, reflexive) or not a closed-class pronoun/determiner.
///
/// The UPOS here is the *default*; [`extract_one_section`] promotes the
/// determiner-like interrogatives / indefinites (those with a gendered
/// `{{Pronomina-Tabelle}}`) from `PRON` to `DET`.
fn wortart_to_pos(wortart: &str) -> Option<(UPOS, PronType)> {
    Some(match wortart {
        "Artikel" => (UPOS::DET, PronType::Art),
        "Demonstrativpronomen" => (UPOS::DET, PronType::Dem),
        "Relativpronomen" => (UPOS::PRON, PronType::Rel),
        "Interrogativpronomen" => (UPOS::PRON, PronType::Int),
        "Indefinitpronomen" => (UPOS::PRON, PronType::Ind),
        _ => return None,
    })
}

/// Extract closed-class pronoun / determiner entries from one page.
///
/// `covered` is the set of lemmas already supplied by the hand-curated
/// closed-class table; entries for those lemmas are skipped so the extractor
/// only adds new coverage.
pub fn extract_pronouns(
    title: &str,
    page_text: &str,
    covered: &HashSet<String>,
) -> Vec<ExtractedEntry> {
    // The lemma itself must be a plain word (rejects clitic article forms
    // like `'n` / `'ne` / `so'n`).
    if covered.contains(title) || clean_cell(title).is_none() {
        return Vec::new();
    }
    let block = match german_block(page_text) {
        Some(b) => b,
        None => return Vec::new(),
    };

    // Inflected-form pages point back to their lemma via Grundformverweis
    // (`alles` → all, `mehr` → viel, `selbiger` → selbig, `das` → der). They
    // are not lemmas, so skip the whole page.
    if block.contains("{{Grundformverweis") {
        return Vec::new();
    }

    let tpls = find_templates(block);

    // Skip pages flagged colloquial / dialectal — these are attested but
    // non-standard (`dat`, `nix`, `bissel`, `büschen`, …) and would pollute
    // standard analysis. See [`looks_nonstandard`].
    if looks_nonstandard(block, &tpls) {
        return Vec::new();
    }

    // Collect the closed-class POS headings and the flexion tables in document
    // order. A heading can list several POS (`{{Wortart|A}}, {{Wortart|B}}`)
    // and a table belongs to the POS heading immediately preceding it.
    let mut marks: Vec<(usize, UPOS, PronType)> = Vec::new();
    let mut tables: Vec<(usize, &Template)> = Vec::new();
    for (i, t) in tpls.iter().enumerate() {
        if t.name == "Wortart" && t.positional.get(1).copied() == Some("Deutsch") {
            if let Some((upos, pt)) = t.positional.first().and_then(|w| wortart_to_pos(w)) {
                marks.push((i, upos, pt));
            }
        } else if t.name == PRONOMINA_TABELLE || t.name == PRONOMEN_UEBERSICHT {
            tables.push((i, t));
        }
    }
    if marks.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();

    if tables.is_empty() {
        // Invariant (indeclinable) pronoun like `allerlei` — no flexion table
        // anywhere in the German block. Only indefinite pronouns have
        // legitimate indeclinable lemmas (the `-lei` family, `…esgleichen`,
        // `frau`). A no-table article / demonstrative / relative page is an
        // oblique or fused form (`alldem`, `derem`, `dessem`, `deretwegen`),
        // not a lemma — drop it. The surface IS the lemma.
        let mut seen = HashSet::new();
        for &(_, upos, pt) in &marks {
            if pt == PronType::Ind && seen.insert((upos, pt)) {
                out.push(make_entry(title.to_string(), title, upos, None, None, None, pt));
            }
        }
        return out;
    }

    // Tabled paradigm(s): attribute each table to the nearest POS heading
    // before it, then emit its cells.
    for &(ti, tbl) in &tables {
        if let Some(&(_, upos, pt)) = marks.iter().rev().find(|(mi, _, _)| *mi < ti) {
            emit_table_cells(title, tbl, upos, pt, &mut out);
        }
    }
    out
}

fn emit_table_cells(
    title: &str,
    tbl: &Template,
    upos: UPOS,
    pron_type: PronType,
    out: &mut Vec<ExtractedEntry>,
) {
    let gendered = tbl.name == PRONOMINA_TABELLE;

    // Citation guard: only the lemma's own page carries the canonical
    // paradigm. Wiktionary also gives inflected forms (`des`, `dem`, `denen`)
    // their own pages with the *same* table; those must not become lemmas.
    // The base cell (Nom Sg masc, or Nom Sg for genderless) must equal the
    // page title.
    let base = if gendered {
        tbl.named_arg("Nominativ Singular m")
            .or_else(|| tbl.named_arg("Nominativ Singular"))
    } else {
        tbl.named_arg("Nominativ Singular")
    };
    match base.and_then(clean_cell) {
        Some(b) if b == title => {}
        _ => return,
    }

    // Determiner-like interrogatives / indefinites with a gendered paradigm
    // (welcher, irgendein, jeglicher) are determiners, not pronouns.
    let upos = match (upos, gendered) {
        (UPOS::PRON, true) if matches!(pron_type, PronType::Int | PronType::Ind) => UPOS::DET,
        (u, _) => u,
    };

    for &(case, case_name) in &CASES {
        if gendered {
            // Singular has three genders; plural is gender-invariant.
            for (gender, gkey) in [
                (Some(Gender::Masc), "m"),
                (Some(Gender::Fem), "f"),
                (Some(Gender::Neut), "n"),
            ] {
                for key in [
                    format!("{case_name} Singular {gkey}"),
                    format!("{case_name} Singular {gkey}*"),
                ] {
                    push_cell(
                        out, title, upos, Some(Number::Sg), gender, case, pron_type,
                        tbl.named_arg(&key),
                    );
                }
            }
            for key in [
                format!("{case_name} Plural"),
                format!("{case_name} Plural*"),
            ] {
                push_cell(
                    out, title, upos, Some(Number::Pl), None, case, pron_type,
                    tbl.named_arg(&key),
                );
            }
        } else {
            for &(number, num_name) in &[(Number::Sg, "Singular"), (Number::Pl, "Plural")] {
                for key in [
                    format!("{case_name} {num_name}"),
                    format!("{case_name} {num_name}*"),
                ] {
                    push_cell(
                        out, title, upos, Some(number), None, case, pron_type,
                        tbl.named_arg(&key),
                    );
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[inline]
fn push_cell(
    out: &mut Vec<ExtractedEntry>,
    title: &str,
    upos: UPOS,
    number: Option<Number>,
    gender: Option<Gender>,
    case: Case,
    pron_type: PronType,
    form: Option<&str>,
) {
    if let Some(clean) = form.and_then(clean_cell) {
        out.push(make_entry(
            clean, title, upos, number, gender, Some(case), pron_type,
        ));
    }
}

fn make_entry(
    surface: String,
    title: &str,
    upos: UPOS,
    number: Option<Number>,
    gender: Option<Gender>,
    case: Option<Case>,
    pron_type: PronType,
) -> ExtractedEntry {
    ExtractedEntry {
        surface,
        lemma: title.to_string(),
        pos: upos,
        features: Features {
            number,
            gender,
            case,
            pron_type: Some(pron_type),
            ..Features::empty()
        },
        source: Source::Attested,
        source_title: title.to_string(),
    }
}

/// Slice out the German-language section of a page: from the first
/// `{{Sprache|Deutsch}}` heading to the next `{{Sprache|…}}` heading (or EOF).
/// Keeps templates from other languages' sections out of scope.
fn german_block(page_text: &str) -> Option<&str> {
    let start = page_text.find("{{Sprache|Deutsch}}")?;
    let rest = &page_text[start..];
    // Next language heading after this one (skip the leading match itself).
    let end = rest[1..]
        .find("{{Sprache|")
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Normalise and validate a single table cell or title into a surface form.
/// Returns `None` for empties, em dashes, markup, or anything not word-like.
/// Conservative on purpose — a dropped cell is better than a corrupt surface.
///
/// Letters only: rejects markup, dash placeholders, multiword cells
/// (`was für`), refs, and the clitic article forms (`'n`, `'ne`, `so'n`) whose
/// apostrophe makes them poor standalone lemmas.
fn clean_cell(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() || !s.chars().all(char::is_alphabetic) {
        return None;
    }
    Some(s.to_string())
}

/// Register / dialect labels that mark a page as non-standard German. Matched
/// only inside marker *forms* — `{{K|…}}` context args, register templates
/// (`{{ugs.|…}}`), `[[wikilinks]]`, and `''italic''` usage notes — never bare
/// prose, so a standard word with e.g. "Berlin" in a citation is unaffected.
const REGISTER: &[&str] = &[
    "ugs",
    "umgangssprachlich",
    "landsch",
    "landschaftlich",
    "mundartlich",
    "mundartnah",
    "mundart",
    "mdal",
    "dialektal",
    "regional",
    "salopp",
    "derb",
    "vulgär",
    "vulg",
    "norddeutsch",
    "süddeutsch",
    "mitteldeutsch",
    "ostmitteldeutsch",
    "westmitteldeutsch",
    "oberdeutsch",
    "niederdeutsch",
    "berlinerisch",
    "hunsrückisch",
    "erzgebirgisch",
    "bairisch",
    "bayrisch",
    "schwäbisch",
    "sächsisch",
    "rheinisch",
    "alemannisch",
    "fränkisch",
    "ruhrdeutsch",
    "kölsch",
    "wienerisch",
    "österreichisch",
    "schweizerisch",
    "kindersprachlich",
];

#[inline]
fn is_register(token: &str) -> bool {
    let t = token.trim().trim_end_matches('.').to_lowercase();
    REGISTER.contains(&t.as_str())
}

/// Detect colloquial / dialectal pages so they stay out of the standard
/// lexicon. Checks register markers in three forms (see [`REGISTER`]):
/// context templates, wikilinks, and italic usage notes.
fn looks_nonstandard(block: &str, tpls: &[Template]) -> bool {
    // (a) Templates: register template names (`{{ugs.|…}}`) and `{{K|…}}` args.
    for t in tpls {
        if is_register(t.name) {
            return true;
        }
        if t.name == "K" && t.positional.iter().any(|a| is_register(a)) {
            return true;
        }
    }
    // (b) Wikilinks `[[term]]` / `[[term|...]]` and (c) italic `''term''` notes.
    let bytes = block.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        let close: &str = match (bytes[i], bytes[i + 1]) {
            (b'[', b'[') => "]]",
            (b'\'', b'\'') => "''",
            _ => {
                i += 1;
                continue;
            }
        };
        let content_start = i + 2; // both markers are 2 ASCII bytes
        let rest = &block[content_start..];
        match rest.find(close) {
            Some(end) => {
                // Split the span on non-letters and test each token. For
                // `[[term|display]]` this yields the target; for italic usage
                // notes (`''norddeutsch:''`, `''[[landschaftlich]]…''`) it
                // yields the dialect adjectives.
                if rest[..end]
                    .split(|c: char| !c.is_alphabetic())
                    .filter(|w| !w.is_empty())
                    .any(is_register)
                {
                    return true;
                }
                i = content_start + end + close.len();
            }
            None => break,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> HashSet<String> {
        HashSet::new()
    }

    fn page_de(section: &str) -> String {
        format!("== headword ({{{{Sprache|Deutsch}}}}) ==\n{section}\n")
    }

    #[test]
    fn invariant_indefinite_allerlei() {
        // allerlei: Indefinitpronomen, no flexion table — the gap we close.
        let text = page_de("=== {{Wortart|Indefinitpronomen|Deutsch}} ===\n:al·ler·lei");
        let e = extract_pronouns("allerlei", &text, &empty());
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].surface, "allerlei");
        assert_eq!(e[0].lemma, "allerlei");
        assert_eq!(e[0].pos, UPOS::PRON);
        assert_eq!(e[0].features.pron_type, Some(PronType::Ind));
        assert_eq!(e[0].features.case, None);
        assert_eq!(e[0].source, Source::Attested);
    }

    #[test]
    fn whole_lei_family_invariant() {
        for w in ["vielerlei", "mancherlei", "zweierlei", "keinerlei"] {
            let text = page_de("=== {{Wortart|Indefinitpronomen|Deutsch}} ===");
            let e = extract_pronouns(w, &text, &empty());
            assert_eq!(e.len(), 1, "{w}");
            assert_eq!(e[0].lemma, w);
            assert_eq!(e[0].features.pron_type, Some(PronType::Ind));
        }
    }

    #[test]
    fn demonstrative_pronomina_tabelle() {
        // A non-closed-class demonstrative with a full gendered paradigm.
        let body = "=== {{Wortart|Demonstrativpronomen|Deutsch}} ===\n\
            {{Pronomina-Tabelle\n\
            |Nominativ Singular m=derjenige\n\
            |Nominativ Singular f=diejenige\n\
            |Nominativ Singular n=dasjenige\n\
            |Nominativ Plural=diejenigen\n\
            |Dativ Singular m=demjenigen\n\
            |Dativ Plural=denjenigen\n\
            }}";
        let e = extract_pronouns("derjenige", &page_de(body), &empty());
        // All cells are DET (demonstrative), Dem, lemma derjenige.
        assert!(e.iter().all(|x| x.pos == UPOS::DET && x.lemma == "derjenige"));
        assert!(e
            .iter()
            .all(|x| x.features.pron_type == Some(PronType::Dem)));
        let nom_m = e
            .iter()
            .find(|x| {
                x.features.case == Some(Case::Nom)
                    && x.features.number == Some(Number::Sg)
                    && x.features.gender == Some(Gender::Masc)
            })
            .unwrap();
        assert_eq!(nom_m.surface, "derjenige");
        let dat_pl = e
            .iter()
            .find(|x| x.features.case == Some(Case::Dat) && x.features.number == Some(Number::Pl))
            .unwrap();
        assert_eq!(dat_pl.surface, "denjenigen");
        assert_eq!(dat_pl.features.gender, None);
    }

    #[test]
    fn gendered_indefinite_promoted_to_det() {
        let body = "=== {{Wortart|Indefinitpronomen|Deutsch}} ===\n\
            {{Pronomina-Tabelle\n\
            |Nominativ Singular m=irgendein\n\
            |Nominativ Singular f=irgendeine\n\
            |Nominativ Singular n=irgendein\n\
            |Akkusativ Singular m=irgendeinen\n\
            }}";
        let e = extract_pronouns("irgendein", &page_de(body), &empty());
        assert!(!e.is_empty());
        // Indefinite + gendered paradigm => determiner.
        assert!(e.iter().all(|x| x.pos == UPOS::DET));
        assert!(e
            .iter()
            .all(|x| x.features.pron_type == Some(PronType::Ind)));
    }

    #[test]
    fn caseonly_uebersicht_stays_pron() {
        // jemand-shaped genderless pronoun (use a non-covered fake lemma).
        let body = "=== {{Wortart|Indefinitpronomen|Deutsch}} ===\n\
            {{Deutsch Pronomen Übersicht\n\
            |Nominativ Singular=werauchimmer\n\
            |Genitiv Singular=werauchimmers\n\
            |Dativ Singular=werauchimmerem\n\
            |Akkusativ Singular=werauchimmeren\n\
            }}";
        let e = extract_pronouns("werauchimmer", &page_de(body), &empty());
        assert_eq!(e.len(), 4);
        assert!(e.iter().all(|x| x.pos == UPOS::PRON && x.features.gender == None));
        let gen_sg = e.iter().find(|x| x.features.case == Some(Case::Gen)).unwrap();
        assert_eq!(gen_sg.surface, "werauchimmers");
    }

    #[test]
    fn inflected_form_page_is_skipped() {
        // The `des` page carries the article table whose Nom Sg m is `der`,
        // not `des` — must not be extracted as a lemma.
        let body = "=== {{Wortart|Artikel|Deutsch}} ===\n\
            {{Pronomina-Tabelle\n\
            |Nominativ Singular m=der\n\
            |Genitiv Singular m=des\n\
            }}";
        let e = extract_pronouns("des", &page_de(body), &empty());
        assert!(e.is_empty(), "inflected-form page should yield nothing: {e:?}");
    }

    #[test]
    fn covered_lemma_is_skipped() {
        // `dieser` is hand-curated in closed_class; extractor must defer.
        let covered: HashSet<String> = ["dieser".to_string()].into_iter().collect();
        let body = "=== {{Wortart|Demonstrativpronomen|Deutsch}} ===\n\
            {{Pronomina-Tabelle\n|Nominativ Singular m=dieser\n}}";
        let e = extract_pronouns("dieser", &page_de(body), &covered);
        assert!(e.is_empty());
    }

    #[test]
    fn star_alternant_emitted() {
        let body = "=== {{Wortart|Demonstrativpronomen|Deutsch}} ===\n\
            {{Pronomina-Tabelle\n\
            |Nominativ Singular m=solch\n\
            |Genitiv Singular m=solches\n\
            |Genitiv Singular m*=solchen\n\
            }}";
        let e = extract_pronouns("solch", &page_de(body), &empty());
        let gen_m: Vec<&str> = e
            .iter()
            .filter(|x| {
                x.features.case == Some(Case::Gen) && x.features.gender == Some(Gender::Masc)
            })
            .map(|x| x.surface.as_str())
            .collect();
        assert!(gen_m.contains(&"solches"));
        assert!(gen_m.contains(&"solchen"));
    }

    #[test]
    fn non_german_section_ignored() {
        let text = "== niemand ({{Sprache|Niederländisch}}) ==\n\
            === {{Wortart|Indefinitpronomen|Niederländisch}} ===";
        let e = extract_pronouns("niemand", text, &empty());
        assert!(e.is_empty());
    }

    #[test]
    fn other_pos_section_does_not_capture_table() {
        // `man`: Adverb section (ignored) then Indefinitpronomen invariant.
        let text = page_de(
            "=== {{Wortart|Adverb|Deutsch}} ===\n:man\n\
             === {{Wortart|Indefinitpronomen|Deutsch}} ===\n:man",
        );
        let e = extract_pronouns("man", &text, &empty());
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].pos, UPOS::PRON);
        assert_eq!(e[0].features.pron_type, Some(PronType::Ind));
        assert_eq!(e[0].features.case, None);
    }

    #[test]
    fn multi_wortart_heading_no_spurious_invariant() {
        // das-like: a multi-POS heading shares one table whose base (der) !=
        // title. The first POS must NOT trigger a bogus invariant for the
        // inflected form; nothing should be emitted.
        let body = "=== {{Wortart|Artikel|Deutsch}}, {{Wortart|Deklinierte Form|Deutsch}} ===\n\
            {{Pronomina-Tabelle\n\
            |Nominativ Singular m=der\n|Nominativ Singular n=das\n|Genitiv Singular m=des\n}}";
        let e = extract_pronouns("das", &page_de(body), &empty());
        assert!(e.is_empty(), "spurious entries: {e:?}");
    }

    #[test]
    fn dialectal_k_template_filtered() {
        // {{K|landschaftlich}} marks a non-standard (regional) entry.
        let body = "=== {{Wortart|Demonstrativpronomen|Deutsch}} ===\n\
            {{Pronomina-Tabelle\n|Nominativ Singular m=dat\n}}\n\
            {{Bedeutungen}}\n:[1] {{K|landschaftlich|hunsrückisch}} [[das]]";
        let e = extract_pronouns("dat", &page_de(body), &empty());
        assert!(e.is_empty(), "dialectal page should be filtered: {e:?}");
    }

    #[test]
    fn italic_dialect_note_filtered() {
        // Register marked only as an italic usage note, not a template.
        let body = "=== {{Wortart|Indefinitpronomen|Deutsch}} ===\n\
            {{Bedeutungen}}\n:[1] ''norddeutsch:'' [[bisschen]]";
        let e = extract_pronouns("büsken", &page_de(body), &empty());
        assert!(e.is_empty(), "italic dialect note should filter: {e:?}");
    }

    #[test]
    fn grundformverweis_page_is_skipped() {
        // `alles` is a Grundformverweis to `all` — an inflected form, not a
        // lemma, even though it has an Indefinitpronomen heading and no table.
        let body = "=== {{Wortart|Indefinitpronomen|Deutsch}} ===\n\
            {{Grundformverweis Dekl|all}}\n:al·les";
        let e = extract_pronouns("alles", &page_de(body), &empty());
        assert!(e.is_empty(), "Grundformverweis page should be skipped: {e:?}");
    }

    #[test]
    fn no_table_demonstrative_is_not_an_invariant() {
        // `alldem` / `derem`: no flexion table, Demonstrativ/Relativ — an
        // oblique/fused form, not a legitimate indeclinable lemma.
        let body = "=== {{Wortart|Demonstrativpronomen|Deutsch}} ===\n:all·dem";
        let e = extract_pronouns("alldem", &page_de(body), &empty());
        assert!(e.is_empty(), "no-table demonstrative should not emit: {e:?}");
    }

    #[test]
    fn region_mentioned_in_plain_prose_not_filtered() {
        // A standard determiner whose page merely mentions a place name in
        // plain prose (no marker form) must NOT be filtered.
        let body = "=== {{Wortart|Indefinitpronomen|Deutsch}} ===\n\
            {{Pronomina-Tabelle\n|Nominativ Singular m=sämtlich\n}}\n\
            {{Beispiele}}\n:[1] Er besuchte sämtliche Museen in Berlin.";
        let e = extract_pronouns("sämtlich", &page_de(body), &empty());
        assert!(!e.is_empty(), "standard word wrongly filtered");
    }
}
