//! Streaming reader for Wikimedia MediaWiki XML dumps.
//!
//! The reader takes a `*-pages-articles.xml.bz2` file, decompresses on the
//! fly (multi-stream-safe via `bzip2::read::MultiBzDecoder`), and yields one
//! `Page` at a time. It is allocation-light in the steady state: a single
//! 8 KiB scratch buffer is reused across events, and the `String` fields
//! on `Page` are taken (not cloned) when the page is emitted.
//!
//! References:
//! - MediaWiki XML export schema:
//!   <https://www.mediawiki.org/wiki/Help:Export>
//! - Wikimedia dump format and multi-stream layout:
//!   <https://meta.wikimedia.org/wiki/Data_dumps/Tools_for_dumps>
//! - quick-xml streaming API (used here):
//!   <https://docs.rs/quick-xml/0.36/quick_xml/reader/struct.Reader.html>

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};
use bzip2::read::MultiBzDecoder;
use quick_xml::events::Event;
use quick_xml::reader::Reader;

/// One MediaWiki page from the dump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub title: String,
    /// MediaWiki namespace. `0` is the main namespace (Wiktionary entries).
    pub ns: i32,
    /// Raw wikitext from the latest revision's `<text>` element.
    pub text: String,
}

impl Page {
    #[inline]
    pub fn is_main_namespace(&self) -> bool {
        self.ns == 0
    }
}

/// Streaming iterator over the pages in a MediaWiki XML dump.
pub struct PageReader<R: BufRead> {
    reader: Reader<R>,
    buf: Vec<u8>,
    state: ScanState,
    title: String,
    ns: i32,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanState {
    Outside,
    InPage,
    InTitle,
    InNs,
    InRevision,
    InText,
}

impl<R: BufRead> PageReader<R> {
    pub fn from_xml_reader(inner: R) -> Self {
        let mut reader = Reader::from_reader(inner);
        reader.config_mut().trim_text(false);
        Self {
            reader,
            buf: Vec::with_capacity(8192),
            state: ScanState::Outside,
            title: String::new(),
            ns: 0,
            text: String::new(),
        }
    }
}

impl PageReader<BufReader<MultiBzDecoder<BufReader<File>>>> {
    /// Open a `.xml.bz2` dump file and return a streaming page reader.
    /// Multi-stream and single-stream bz2 are both handled.
    pub fn open_bz2(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        // BufReader<File> before MultiBzDecoder because MultiBzDecoder
        // requires BufRead; BufReader after the decoder for quick-xml's
        // read_event_into buffer hits.
        let file_buf = BufReader::with_capacity(1 << 16, file);
        let decoder = MultiBzDecoder::new(file_buf);
        let xml_buf = BufReader::with_capacity(1 << 20, decoder);
        Ok(Self::from_xml_reader(xml_buf))
    }
}

impl<R: BufRead> Iterator for PageReader<R> {
    type Item = Result<Page>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.buf.clear();
            let event = match self.reader.read_event_into(&mut self.buf) {
                Ok(e) => e,
                Err(e) => return Some(Err(anyhow::anyhow!("xml read error: {}", e))),
            };
            match event {
                Event::Eof => return None,

                Event::Start(start) => match start.name().as_ref() {
                    b"page" if self.state == ScanState::Outside => {
                        self.state = ScanState::InPage;
                        self.title.clear();
                        self.text.clear();
                        self.ns = 0;
                    }
                    b"title" if self.state == ScanState::InPage => {
                        self.state = ScanState::InTitle;
                    }
                    b"ns" if self.state == ScanState::InPage => {
                        self.state = ScanState::InNs;
                    }
                    b"revision" if self.state == ScanState::InPage => {
                        self.state = ScanState::InRevision;
                    }
                    b"text" if self.state == ScanState::InRevision => {
                        self.state = ScanState::InText;
                    }
                    _ => {}
                },

                Event::End(end) => match end.name().as_ref() {
                    b"page" if self.state == ScanState::InPage => {
                        self.state = ScanState::Outside;
                        return Some(Ok(Page {
                            title: std::mem::take(&mut self.title),
                            ns: self.ns,
                            text: std::mem::take(&mut self.text),
                        }));
                    }
                    b"title" if self.state == ScanState::InTitle => {
                        self.state = ScanState::InPage;
                    }
                    b"ns" if self.state == ScanState::InNs => {
                        self.state = ScanState::InPage;
                    }
                    b"revision" if self.state == ScanState::InRevision => {
                        self.state = ScanState::InPage;
                    }
                    b"text" if self.state == ScanState::InText => {
                        self.state = ScanState::InRevision;
                    }
                    _ => {}
                },

                Event::Text(t) => match self.state {
                    ScanState::InTitle => {
                        if let Ok(s) = t.unescape() {
                            self.title.push_str(&s);
                        }
                    }
                    ScanState::InNs => {
                        if let Ok(s) = t.unescape() {
                            if let Ok(n) = s.trim().parse::<i32>() {
                                self.ns = n;
                            }
                        }
                    }
                    ScanState::InText => {
                        if let Ok(s) = t.unescape() {
                            self.text.push_str(&s);
                        }
                    }
                    _ => {}
                },

                Event::CData(c) => {
                    if self.state == ScanState::InText {
                        self.text.push_str(&String::from_utf8_lossy(&c));
                    }
                }

                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    const SAMPLE: &str = r#"<mediawiki>
  <page>
    <title>Tisch</title>
    <ns>0</ns>
    <revision>
      <text>== Tisch ({{Sprache|Deutsch}}) ==
=== {{Wortart|Substantiv|Deutsch}}, {{m}} ===
{{Deutsch Substantiv Übersicht
|Genus=m
|Nominativ Singular=Tisch
|Nominativ Plural=Tische
}}</text>
    </revision>
  </page>
  <page>
    <title>Diskussion:Tisch</title>
    <ns>1</ns>
    <revision>
      <text>some discussion</text>
    </revision>
  </page>
  <page>
    <title>Haus</title>
    <ns>0</ns>
    <revision>
      <text>== Haus ({{Sprache|Deutsch}}) ==</text>
    </revision>
  </page>
</mediawiki>"#;

    #[test]
    fn iterates_all_pages() {
        let reader = PageReader::from_xml_reader(Cursor::new(SAMPLE));
        let pages: Result<Vec<_>> = reader.collect();
        let pages = pages.unwrap();
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0].title, "Tisch");
        assert_eq!(pages[0].ns, 0);
        assert!(pages[0].text.contains("Deutsch Substantiv Übersicht"));
        assert_eq!(pages[1].title, "Diskussion:Tisch");
        assert_eq!(pages[1].ns, 1);
        assert_eq!(pages[2].title, "Haus");
    }

    #[test]
    fn filter_main_namespace() {
        let reader = PageReader::from_xml_reader(Cursor::new(SAMPLE));
        let pages: Vec<Page> = reader
            .filter_map(|p| p.ok())
            .filter(|p| p.is_main_namespace())
            .collect();
        assert_eq!(pages.len(), 2);
        assert!(pages.iter().all(|p| p.ns == 0));
    }

    #[test]
    fn umlauts_in_title_and_text() {
        let xml = r#"<mediawiki><page><title>Müller</title><ns>0</ns>
            <revision><text>Größe und straße</text></revision></page></mediawiki>"#;
        let mut reader = PageReader::from_xml_reader(Cursor::new(xml));
        let page = reader.next().unwrap().unwrap();
        assert_eq!(page.title, "Müller");
        assert_eq!(page.text, "Größe und straße");
    }

    #[test]
    fn xml_entities_unescaped_in_text() {
        let xml = r#"<mediawiki><page><title>T</title><ns>0</ns>
            <revision><text>A &amp; B &lt; C</text></revision></page></mediawiki>"#;
        let mut reader = PageReader::from_xml_reader(Cursor::new(xml));
        let page = reader.next().unwrap().unwrap();
        assert_eq!(page.text, "A & B < C");
    }
}
