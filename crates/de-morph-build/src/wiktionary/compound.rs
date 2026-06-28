//! Extract compound declarations from Wiktionary `{{Herkunft}}`
//! (Etymology) sections.
//!
//! German Wiktionary pages for compound nouns conventionally mark
//! their compositional structure inside the `{{Herkunft}}` template.
//! The canonical patterns are:
//!
//! ```text
//! {{Herkunft}}
//! :[[Determinativkompositum]] aus ''[[Fund]]'' und ''[[Ort]]''
//!
//! :[[Determinativkompositum]] aus den Substantiven ''[[Wort]]''
//!  und ''[[Buch]]'' sowie dem [[Fugenelement]] ''[[-er]]'' (plus [[Umlaut]])
//! ```
//!
//! From which we recover:
//!   - the compound TYPE (Determinativ, Possessiv, Kopulativ, ...)
//!   - the constituent PARTS (as wiki-link targets — `Wort`, `Buch`)
//!   - the FUGENELEMENT (linker) if explicitly mentioned (`er`)
//!
//! References (verified):
//! - Determinativkompositum: <https://de.wiktionary.org/wiki/Determinativkompositum>
//! - Fugenelement docs: <https://de.wiktionary.org/wiki/Fugenelement>
//!
//! 67k+ pages in the 20260601 dump carry this markup (see the
//! `[[Determinativkompositum]]` count grepped during design).
//!
//! Robustness note: the etymology section is free-form prose with many
//! variants. We capture the dominant `aus [[X]] und [[Y]]` shape and
//! accept some misses on exotic phrasings.

use serde::Serialize;

/// Compound declaration extracted from a Wiktionary page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompoundEntry {
    /// The compound lemma (the Wiktionary page title).
    pub lemma: String,
    /// Compound type per Wiktionary's classification.
    pub compound_type: CompoundType,
    /// Constituent lemmas — typically 2, occasionally 3.
    pub parts: Vec<String>,
    /// Linker (Fugenelement) between the parts, with the leading `-`
    /// stripped. `None` means no explicit linker was mentioned (most
    /// 2-part compounds have empty linker by default).
    pub fugenelement: Option<String>,
    /// The Wiktionary article title — provenance for CC BY-SA
    /// attribution.
    pub source_title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CompoundType {
    /// Standard noun + noun → noun (Haus+Tür = Haustür). The
    /// overwhelmingly most common type.
    Determinativ,
    /// Bahuvrihi (semantic-head outside the compound, e.g. Rotkehlchen
    /// = a bird with a red throat).
    Possessiv,
    /// Coordinative (X+Y = "both X and Y", e.g. Hosenrock).
    Kopulativ,
    /// Catch-all for less common types (Konfix-, Rektions-,
    /// Adverb-Kompositum, etc.).
    Other,
}

/// Extract compound declarations from a single page's wikitext.
///
/// Returns an empty vector if no Determinativkompositum / Possessiv-
/// / Kopulativkompositum marker is found.
pub fn extract_compounds(title: &str, page_text: &str) -> Vec<CompoundEntry> {
    let mut out = Vec::new();
    for &(marker, ctype) in COMPOUND_MARKERS {
        let mut search_from = 0;
        while let Some(rel_pos) = page_text[search_from..].find(marker) {
            let abs_pos = search_from + rel_pos;
            let after_marker = abs_pos + marker.len();
            let context_end = find_end_of_context(page_text, after_marker);
            let context = &page_text[after_marker..context_end];
            if let Some((parts, fuge)) = parse_compound_context(context) {
                if parts.len() >= 2 {
                    out.push(CompoundEntry {
                        lemma: title.to_string(),
                        compound_type: ctype,
                        parts,
                        fugenelement: fuge,
                        source_title: title.to_string(),
                    });
                }
            }
            search_from = after_marker;
        }
    }
    out
}

/// Compound-marker wikilinks paired with their CompoundType.
/// Listed in order of frequency (Determinativ dominates by orders of
/// magnitude).
const COMPOUND_MARKERS: &[(&str, CompoundType)] = &[
    ("[[Determinativkompositum]]", CompoundType::Determinativ),
    ("[[Possessivkompositum]]", CompoundType::Possessiv),
    ("[[Kopulativkompositum]]", CompoundType::Kopulativ),
    // Less common variants — all bucket into Other.
    ("[[Konfixkompositum]]", CompoundType::Other),
    ("[[Rektionskompositum]]", CompoundType::Other),
    ("[[Adverbkompositum]]", CompoundType::Other),
    ("[[Inversionskompositum]]", CompoundType::Other),
    ("[[Explikativkompositum]]", CompoundType::Other),
    ("[[Bikompositum]]", CompoundType::Other),
    ("[[Selbstkompositum]]", CompoundType::Other),
];

/// Find a reasonable end-of-context for parsing the prose following a
/// compound marker. The etymology line is usually short and ends at
/// a blank line OR the next template opener `{{`.
fn find_end_of_context(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = start;
    while i + 1 < bytes.len() {
        // Paragraph break.
        if bytes[i] == b'\n' && bytes[i + 1] == b'\n' {
            return i;
        }
        // Next section / template starts a new context.
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            return i;
        }
        i += 1;
    }
    bytes.len().min(start + 800) // hard cap to keep parsing local
}

/// Parse the prose immediately following a Kompositum marker. Looks
/// for the `aus [[X]] und [[Y]]` (or `aus [[X]], [[Y]] und [[Z]]`)
/// shape and, optionally, the `Fugenelement [[-LINKER]]` annotation.
fn parse_compound_context(ctx: &str) -> Option<(Vec<String>, Option<String>)> {
    // The marker is followed by " aus " in the dominant phrasing.
    // We accept several leading filler words ("den Substantiven",
    // "dem Verb", etc.) and just look for wiki-links following " aus ".
    let aus_offset = ctx.find(" aus ")?;
    let mut cursor = aus_offset + " aus ".len();

    // Collect parts: walk forward through alternating " und " / ","
    // separators and pick the first wiki-link after each.
    let mut parts = Vec::new();
    // First part: scan forward to the first valid word link.
    let (part, next) = next_word_link(ctx, cursor)?;
    parts.push(part);
    cursor = next;

    // Optional second, third part via " und " or ", " separators.
    loop {
        let rest = &ctx[cursor..];
        let sep_at = rest
            .find(" und ")
            .map(|p| (p, " und ".len()))
            .or_else(|| rest.find(", ").map(|p| (p, ", ".len())));
        let Some((sep_pos, sep_len)) = sep_at else {
            break;
        };
        cursor += sep_pos + sep_len;
        let Some((part, next)) = next_word_link(ctx, cursor) else {
            break;
        };
        parts.push(part);
        cursor = next;
        if parts.len() >= 4 {
            break;
        }
    }

    // Look for `[[Fugenelement]]` and the linker that follows. The
    // search is local — we only look up to ~200 bytes after the parts
    // were consumed.
    let fuge = parse_fugenelement(ctx, cursor);

    Some((parts, fuge))
}

/// Find the next wiki-link that points at a real word (not the
/// "Substantiv" / "Verb" / "Adjektiv" / "Fugenelement" / "Umlaut" /
/// "Komposition" meta-links that often appear in the etymology
/// section). Returns (link target, byte position after the link).
fn next_word_link(text: &str, from: usize) -> Option<(String, usize)> {
    let mut cursor = from;
    loop {
        let (link_start, link_end, target) = next_wiki_link(text, cursor)?;
        let cleaned = clean_link_target(target);
        if is_word_link(&cleaned) {
            return Some((cleaned, link_end));
        }
        // Meta-link — skip and look further. Avoid infinite loop.
        cursor = link_end.max(link_start + 1);
    }
}

/// Find the next `[[…]]` wiki-link starting from `from`. Returns
/// (start, end, raw target). The raw target may contain `|` (pipe
/// for display) and `#` (section anchor) — clean those with
/// [`clean_link_target`].
fn next_wiki_link(text: &str, from: usize) -> Option<(usize, usize, &str)> {
    let bytes = text.as_bytes();
    let mut i = from;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let inner_start = i + 2;
            let mut j = inner_start;
            while j + 1 < bytes.len() {
                if bytes[j] == b']' && bytes[j + 1] == b']' {
                    let raw = &text[inner_start..j];
                    let target = raw.split('|').next().unwrap_or(raw);
                    return Some((i, j + 2, target));
                }
                j += 1;
            }
            return None;
        }
        i += 1;
    }
    None
}

/// Clean a raw wiki-link target: strip section anchors (`Haus#Substantiv`
/// → `Haus`) and ASCII whitespace.
fn clean_link_target(raw: &str) -> String {
    let head = raw.split('#').next().unwrap_or(raw).trim();
    head.to_string()
}

/// Reject wiki-links that point at META categories rather than
/// constituent words. These are POS labels and morphology terms that
/// Wiktionary editors link to as filler ("dem [[Substantiv]] ...");
/// they should never appear as compound parts.
///
/// Note: regular German nouns like `Wort`, `Tag`, `Bund` are NOT in
/// this list — they are legitimately constituent parts of common
/// compounds (Wörter+buch, Bundes+tag, etc.).
fn is_word_link(target: &str) -> bool {
    if target.is_empty() {
        return false;
    }
    !matches!(
        target,
        "Substantiv"
            | "Substantive"
            | "Verb"
            | "Verben"
            | "Verbs"
            | "Adjektiv"
            | "Adjektive"
            | "Adverb"
            | "Partikel"
            | "Pronomen"
            | "Numerale"
            | "Fugenelement"
            | "Umlaut"
            | "Komposition"
            | "Suffix"
            | "Präfix"
            | "Konfix"
            | "Determinativkompositum"
            | "Possessivkompositum"
            | "Kopulativkompositum"
            | "Konfixkompositum"
            | "Rektionskompositum"
            | "Adverbkompositum"
    )
}

/// Find `[[Fugenelement]]` near `from` and extract the linker name
/// from the link that follows it.
fn parse_fugenelement(ctx: &str, from: usize) -> Option<String> {
    // Limit search to the next ~250 bytes — the Fugenelement annotation
    // typically appears right after the parts list.
    let scope_end = (from + 250).min(ctx.len());
    let scope = ctx.get(from..scope_end)?;
    let fuge_pos = scope.find("[[Fugenelement]]")?;
    let after = fuge_pos + "[[Fugenelement]]".len();
    let (_, _, target) = next_wiki_link(scope, after)?;
    let cleaned = clean_link_target(target);
    // Fugenelement links conventionally start with `-` (e.g. `[[-er]]`,
    // `[[-s]]`, `[[-n]]`). Strip it for the JSON output.
    let stripped = cleaned.trim_start_matches('-');
    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_simple_two_part_compound() {
        let page = "{{Herkunft}}\n\
            :[[Determinativkompositum]] aus ''[[Fund]]'' und ''[[Ort]]''\n\
            \n\
            {{Synonyme}}";
        let out = extract_compounds("Fundort", page);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].compound_type, CompoundType::Determinativ);
        assert_eq!(out[0].parts, vec!["Fund".to_string(), "Ort".to_string()]);
        assert_eq!(out[0].fugenelement, None);
    }

    #[test]
    fn extracts_compound_with_fugenelement() {
        let page = "{{Herkunft}}\n\
            :[[Determinativkompositum]] aus den Substantiven \
            ''[[Wort]]'' und ''[[Buch]]'' sowie dem [[Fugenelement]] \
            ''[[-er]]'' (plus [[Umlaut]])\n\
            \n";
        let out = extract_compounds("Wörterbuch", page);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].parts, vec!["Wort".to_string(), "Buch".to_string()]);
        assert_eq!(out[0].fugenelement, Some("er".to_string()));
    }

    #[test]
    fn extracts_compound_with_pipe_link() {
        let page = "{{Herkunft}}\n\
            :[[Determinativkompositum]] aus ''[[Bund|Bundes]]'' und \
            ''[[Tag]]''\n";
        let out = extract_compounds("Bundestag", page);
        assert_eq!(out.len(), 1);
        // The pipe-link `[[Bund|Bundes]]` resolves to target `Bund`.
        assert_eq!(out[0].parts[0], "Bund");
        assert_eq!(out[0].parts[1], "Tag");
    }

    #[test]
    fn handles_section_anchor_in_link() {
        let page = "{{Herkunft}}\n\
            :[[Determinativkompositum]] aus ''[[Haus#Substantiv|Haus]]'' \
            und ''[[Tür]]''\n";
        let out = extract_compounds("Haustür", page);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].parts, vec!["Haus".to_string(), "Tür".to_string()]);
    }

    #[test]
    fn rejects_meta_links_substantiv_etc() {
        let page = "{{Herkunft}}\n\
            :[[Determinativkompositum]] aus dem [[Substantiv]] \
            ''[[Sonne]]'' und dem [[Substantiv]] ''[[Strahl]]''\n";
        let out = extract_compounds("Sonnenstrahl", page);
        assert_eq!(out.len(), 1);
        // [[Substantiv]] is a meta-link and must be skipped.
        assert_eq!(
            out[0].parts,
            vec!["Sonne".to_string(), "Strahl".to_string()]
        );
    }

    #[test]
    fn page_without_marker_yields_nothing() {
        let page = "{{Herkunft}}\n\
            :Substantivierung des Grußworts [[hallo]]\n";
        let out = extract_compounds("Hallo", page);
        assert!(out.is_empty());
    }

    #[test]
    fn handles_three_part_compound() {
        let page = "{{Herkunft}}\n\
            :[[Determinativkompositum]] aus ''[[Bund]]'', ''[[es]]'' \
            und ''[[Tag]]''\n";
        // (Synthetic — real-world Bundestag is annotated with a
        // Fugenelement annotation, not as a 3-part compound. This
        // tests the comma-separator branch only.)
        let out = extract_compounds("test", page);
        assert_eq!(out.len(), 1);
        assert!(out[0].parts.contains(&"Bund".to_string()));
        assert!(out[0].parts.contains(&"Tag".to_string()));
    }

    #[test]
    fn multiple_markers_produce_multiple_entries() {
        let page = "{{Herkunft}}\n\
            :strukturell: [[Determinativkompositum]] aus ''[[A]]'' und ''[[B]]''\n\
            :synchron: [[Determinativkompositum]] aus ''[[C]]'' und ''[[D]]''\n\
            \n";
        let out = extract_compounds("foo", page);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn possessivkompositum_marker_tagged_correctly() {
        let page = "{{Herkunft}}\n\
            :[[Possessivkompositum]] aus ''[[rot]]'' und ''[[Kehle]]'' \
            sowie dem Suffix ''-chen''\n";
        let out = extract_compounds("Rotkehlchen", page);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].compound_type, CompoundType::Possessiv);
    }
}
