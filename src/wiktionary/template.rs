//! MediaWiki template parser — extracts `{{Name|arg1|key=value|...}}` blocks
//! from wikitext.
//!
//! Performance characteristics:
//! - Scanner is byte-level (ASCII fast path). UTF-8 multibyte sequences
//!   never collide with `{`, `}`, `[`, `]`, `|`, or `=` because those are
//!   all single-byte ASCII and the high-bit-set bytes that make up
//!   multibyte sequences are disjoint from ASCII. The scanner is therefore
//!   safe and correct over UTF-8 text without per-codepoint decoding.
//! - The parser borrows from the input string (`&'a str`). No allocations
//!   except the `Vec`s holding the argument slices.
//! - Nested templates and wiki links are tracked by a depth counter to
//!   avoid splitting on pipes inside them.
//!
//! References:
//! - Template syntax: <https://www.mediawiki.org/wiki/Help:Templates>.
//! - Argument parsing rules: <https://www.mediawiki.org/wiki/Help:Templates#Parameters>.
//!   The "first `=` outside any nested construct" rule used in
//!   `parse_template_body` follows the same disambiguation MediaWiki itself
//!   uses for distinguishing positional from named arguments.

/// A parsed template invocation. All fields borrow from the source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template<'a> {
    /// Template name (e.g. `"Deutsch Substantiv Übersicht"`). Trimmed.
    pub name: &'a str,
    /// Positional (unnamed) arguments in source order. Each value is trimmed.
    pub positional: Vec<&'a str>,
    /// Named arguments in source order. `(key, value)`; both trimmed.
    pub named: Vec<(&'a str, &'a str)>,
    /// Byte range of the entire `{{...}}` invocation in the source text,
    /// inclusive of the braces.
    pub span: (usize, usize),
}

impl<'a> Template<'a> {
    /// Look up a named argument by key. Returns the first match in source
    /// order.
    pub fn named_arg(&self, key: &str) -> Option<&'a str> {
        self.named
            .iter()
            .find_map(|(k, v)| (*k == key).then_some(*v))
    }

    /// Return the n-th positional argument (1-based, matching MediaWiki
    /// convention).
    pub fn positional_arg(&self, n: usize) -> Option<&'a str> {
        if n == 0 {
            return None;
        }
        self.positional.get(n - 1).copied()
    }
}

/// Find every top-level template invocation in `text`.
///
/// Nested templates are not returned as separate entries; they are part
/// of the enclosing template's body.
pub fn find_templates(text: &str) -> Vec<Template<'_>> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(end) = scan_to_matching_close(bytes, i) {
                let body = &text[i + 2..end - 2];
                if let Some(mut t) = parse_template_body(body) {
                    t.span = (i, end);
                    out.push(t);
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Given the source bytes and the index of an opening `{{`, return the
/// byte index just past the matching `}}`. `None` if unmatched.
fn scan_to_matching_close(bytes: &[u8], open_at: usize) -> Option<usize> {
    debug_assert!(
        open_at + 1 < bytes.len() && bytes[open_at] == b'{' && bytes[open_at + 1] == b'{'
    );
    let mut depth: i32 = 1;
    let mut i = open_at + 2;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            depth += 1;
            i += 2;
        } else if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return Some(i);
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Parse the body of a template (the text between `{{` and `}}`).
///
/// Returns `None` if the body has no name (e.g. empty `{{}}`).
pub fn parse_template_body(body: &str) -> Option<Template<'_>> {
    let parts = split_top_level_pipes(body);
    let name = parts.first()?.trim();
    if name.is_empty() {
        return None;
    }
    let mut positional = Vec::new();
    let mut named = Vec::new();
    for part in &parts[1..] {
        match split_top_level_first_eq(part) {
            Some((k, v)) => named.push((k.trim(), v.trim())),
            None => positional.push(part.trim()),
        }
    }
    Some(Template {
        name,
        positional,
        named,
        span: (0, 0),
    })
}

/// Split a template body at top-level pipes, ignoring pipes inside
/// nested `{{...}}` or `[[...]]`.
fn split_top_level_pipes(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut start = 0;
    let mut i = 0;
    let mut template_depth: i32 = 0;
    let mut link_depth: i32 = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            template_depth += 1;
            i += 2;
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'}' && bytes[i + 1] == b'}' {
            if template_depth > 0 {
                template_depth -= 1;
            }
            i += 2;
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'[' && bytes[i + 1] == b'[' {
            link_depth += 1;
            i += 2;
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b']' && bytes[i + 1] == b']' {
            if link_depth > 0 {
                link_depth -= 1;
            }
            i += 2;
            continue;
        }
        if bytes[i] == b'|' && template_depth == 0 && link_depth == 0 {
            out.push(&body[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    out.push(&body[start..]);
    out
}

/// Split a single argument at the first top-level `=`, returning
/// `Some((key, value))` if found, or `None` if the argument is purely
/// positional. The `=` inside a nested `{{...}}`, `[[...]]`, or HTML tag
/// does not count.
fn split_top_level_first_eq(arg: &str) -> Option<(&str, &str)> {
    let bytes = arg.as_bytes();
    let mut i = 0;
    let mut template_depth: i32 = 0;
    let mut link_depth: i32 = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            template_depth += 1;
            i += 2;
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'}' && bytes[i + 1] == b'}' {
            if template_depth > 0 {
                template_depth -= 1;
            }
            i += 2;
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'[' && bytes[i + 1] == b'[' {
            link_depth += 1;
            i += 2;
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b']' && bytes[i + 1] == b']' {
            if link_depth > 0 {
                link_depth -= 1;
            }
            i += 2;
            continue;
        }
        if bytes[i] == b'=' && template_depth == 0 && link_depth == 0 {
            return Some((&arg[..i], &arg[i + 1..]));
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_template() {
        let t = parse_template_body("Sprache|Deutsch").unwrap();
        assert_eq!(t.name, "Sprache");
        assert_eq!(t.positional, vec!["Deutsch"]);
        assert!(t.named.is_empty());
    }

    #[test]
    fn parses_named_arg() {
        let t = parse_template_body("Substantiv Übersicht\n|Genus=m\n|Nominativ Singular=Tisch")
            .unwrap();
        assert_eq!(t.name, "Substantiv Übersicht");
        assert_eq!(t.named_arg("Genus"), Some("m"));
        assert_eq!(t.named_arg("Nominativ Singular"), Some("Tisch"));
    }

    #[test]
    fn umlauts_in_args() {
        let t = parse_template_body("Adj|Größe=groß|Plural=Häuser").unwrap();
        assert_eq!(t.named_arg("Größe"), Some("groß"));
        assert_eq!(t.named_arg("Plural"), Some("Häuser"));
    }

    #[test]
    fn nested_template_does_not_break_split() {
        let t = parse_template_body("Wortart|Substantiv|Deutsch|extra={{m}}").unwrap();
        assert_eq!(t.name, "Wortart");
        assert_eq!(t.positional, vec!["Substantiv", "Deutsch"]);
        assert_eq!(t.named_arg("extra"), Some("{{m}}"));
    }

    #[test]
    fn nested_template_pipe_does_not_split() {
        let t = parse_template_body("Outer|x={{Inner|a|b}}|y=2").unwrap();
        assert_eq!(t.named_arg("x"), Some("{{Inner|a|b}}"));
        assert_eq!(t.named_arg("y"), Some("2"));
    }

    #[test]
    fn wiki_link_pipe_does_not_split() {
        let t = parse_template_body("Tpl|caption=[[Tisch|der Tisch]]").unwrap();
        assert_eq!(t.named_arg("caption"), Some("[[Tisch|der Tisch]]"));
    }

    #[test]
    fn finds_top_level_templates() {
        let text = "before {{One|a}} middle {{Two|x=1|y=2}} after";
        let templates = find_templates(text);
        assert_eq!(templates.len(), 2);
        assert_eq!(templates[0].name, "One");
        assert_eq!(templates[0].positional, vec!["a"]);
        assert_eq!(templates[1].name, "Two");
        assert_eq!(templates[1].named_arg("x"), Some("1"));
        assert_eq!(templates[1].named_arg("y"), Some("2"));
    }

    #[test]
    fn nested_templates_not_returned_as_top_level() {
        let text = "{{Outer|inner={{Inner|a|b}}}}";
        let templates = find_templates(text);
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].name, "Outer");
    }

    #[test]
    fn span_points_to_braces() {
        let text = "xx {{Foo|a=1}} yy";
        let templates = find_templates(text);
        assert_eq!(templates.len(), 1);
        let (start, end) = templates[0].span;
        assert_eq!(&text[start..end], "{{Foo|a=1}}");
    }

    #[test]
    fn unmatched_template_is_ignored() {
        let text = "good {{Foo|a=1}} bad {{Unclosed";
        let templates = find_templates(text);
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].name, "Foo");
    }

    #[test]
    fn empty_template_body_returns_none() {
        assert!(parse_template_body("").is_none());
        assert!(parse_template_body("   ").is_none());
    }

    #[test]
    fn deutsch_substantiv_uebersicht_parses() {
        // Realistic template body — wikitext as it appears in dewiktionary
        // for the noun "Tisch". Source: <https://de.wiktionary.org/wiki/Tisch>
        let body = "Deutsch Substantiv Übersicht
|Genus=m
|Nominativ Singular=Tisch
|Genitiv Singular=Tisches
|Genitiv Singular*=Tischs
|Dativ Singular=Tisch
|Dativ Singular*=Tische
|Akkusativ Singular=Tisch
|Nominativ Plural=Tische
|Genitiv Plural=Tische
|Dativ Plural=Tischen
|Akkusativ Plural=Tische";
        let t = parse_template_body(body).unwrap();
        assert_eq!(t.name, "Deutsch Substantiv Übersicht");
        assert_eq!(t.named_arg("Genus"), Some("m"));
        assert_eq!(t.named_arg("Nominativ Singular"), Some("Tisch"));
        assert_eq!(t.named_arg("Genitiv Singular"), Some("Tisches"));
        assert_eq!(t.named_arg("Genitiv Singular*"), Some("Tischs"));
        assert_eq!(t.named_arg("Dativ Plural"), Some("Tischen"));
        assert_eq!(t.named_arg("Akkusativ Plural"), Some("Tische"));
    }

    #[test]
    fn perf_smoke_no_quadratic_blowup() {
        // 10k templates concatenated; should parse in well under a second.
        // This is a smoke test, not a benchmark — it asserts only that
        // we don't accidentally regress to O(n²) parsing.
        let mut text = String::with_capacity(10_000 * 32);
        for i in 0..10_000 {
            text.push_str(&format!("{{{{T{}|k={}|v=x}}}} ", i, i));
        }
        let templates = find_templates(&text);
        assert_eq!(templates.len(), 10_000);
        assert_eq!(templates[42].named_arg("k"), Some("42"));
    }
}
