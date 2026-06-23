//! Build a lexicon from `(surface, lemma, pos, features, source)` records.
//!
//! Two artefacts are written:
//! - the FST file (surface → packed u64 pointer)
//! - the side-table file (header + lemma intern table + analyses)
//!
//! Records do not need to be sorted in input order; the builder sorts
//! by surface internally to satisfy the FST builder's contract. For
//! very large inputs an external sort would be needed; at ~1.4M
//! records (the nouns + verbs lexicon) the in-memory sort fits
//! comfortably and finishes in under a second.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;

use fst::MapBuilder;

use crate::analysis::{Aux, Features, PackedFeatures, Source, UPOS};
use crate::lexicon::format::{
    pack_fst_value, AnalysisRecord, Shape, ANALYSIS_RECORD_SIZE, HEADER_SIZE, MAGIC, MAX_SHAPE_ID,
    SHAPE_ENTRY_SIZE, VERSION_MAJOR, VERSION_MINOR,
};

/// Error returned by [`LexiconBuilder::finish`].
#[derive(Debug)]
pub enum BuildError {
    /// I/O error while writing one of the output streams.
    Io(std::io::Error),
    /// The FST builder rejected an entry (typically: keys not sorted).
    Fst(fst::Error),
    /// More than `u32::MAX` lemmas — out-of-range for the format.
    TooManyLemmas,
    /// More than `u32::MAX` analyses — out-of-range for the format.
    TooManyAnalyses,
    /// A single surface has more than `u32::MAX` analyses.
    TooManyAnalysesPerSurface,
    /// More distinct analysis shapes than the 12-bit `shape_id` allows.
    TooManyShapes,
    /// Side-table size exceeds `u32::MAX` bytes.
    SideTableTooLarge,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Fst(e) => write!(f, "fst error: {e}"),
            Self::TooManyLemmas => write!(f, "more than u32::MAX lemmas"),
            Self::TooManyAnalyses => write!(f, "more than u32::MAX analyses"),
            Self::TooManyAnalysesPerSurface => {
                write!(f, "more than u32::MAX analyses for one surface")
            }
            Self::TooManyShapes => write!(f, "more than 4096 distinct analysis shapes"),
            Self::SideTableTooLarge => write!(f, "side table exceeds 4 GiB"),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<std::io::Error> for BuildError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<fst::Error> for BuildError {
    fn from(e: fst::Error) -> Self {
        Self::Fst(e)
    }
}

/// Whether a surface belongs in the analyzer FST. Rejects:
///
/// - **Any whitespace.** The analyzer is keyed on single whitespace
///   tokens, so a surface containing a space is multi-token and can
///   never match a single input token: `zu lieben` (look up `zu` and
///   `lieben` separately), multiword lemmas (`Sicherheitsrat der
///   Vereinten Nationen`), `so genannte`, the separated forms of
///   separable verbs (`tauche ab`). The single-word zu-infinitive of a
///   separable verb (`abzutauchen`) has no space and is kept.
/// - **Leaked wikitext/HTML markup** the template parser passed through:
///   HTML comments (`isometrischer <!--…-->`), `<small>`/`<ref>` tags,
///   doubled braces/brackets.
///
/// Single-bracket punctuation entries (`[`, `]`, `{`, `}`) are kept —
/// only *doubled* braces/brackets signal markup.
pub fn is_clean_surface(surface: &str) -> bool {
    !(surface.contains(char::is_whitespace)
        || surface.contains('<')
        || surface.contains('>')
        || surface.contains("{{")
        || surface.contains("}}")
        || surface.contains("[[")
        || surface.contains("]]"))
}

/// Streaming builder. Records are added in arbitrary order; sorting
/// and FST/side-table emission happens in [`LexiconBuilder::finish`].
#[derive(Default)]
pub struct LexiconBuilder {
    /// Collected records grouped by surface form for sorting.
    by_surface: BTreeMap<String, Vec<PendingRecord>>,
    /// Lemma intern table: lemma string → lemma_id.
    lemma_ids: HashMap<String, u32>,
    /// Lemmas in insertion order so we can write them sequentially.
    lemmas: Vec<String>,
    total_records: u64,
}

#[derive(Debug, Clone)]
struct PendingRecord {
    lemma_id: u32,
    pos: UPOS,
    source: Source,
    features: PackedFeatures,
    /// Auxiliary code (0=unset/1=Haben/2=Sein/3=Both) — carried
    /// separately because `PackedFeatures` is full and can't hold it.
    aux: u8,
}

impl LexiconBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn total_records(&self) -> u64 {
        self.total_records
    }

    pub fn num_lemmas(&self) -> usize {
        self.lemmas.len()
    }

    pub fn num_surfaces(&self) -> usize {
        self.by_surface.len()
    }

    /// Add one record to the lexicon.
    ///
    /// The `surface` is the FST key; `lemma` is interned into the
    /// shared table on first encounter. Identical (surface, lemma,
    /// pos, features, source) tuples are de-duplicated.
    pub fn add(
        &mut self,
        surface: &str,
        lemma: &str,
        pos: UPOS,
        features: Features,
        source: Source,
    ) -> Result<(), BuildError> {
        let lemma_id = match self.lemma_ids.get(lemma) {
            Some(&id) => id,
            None => {
                let id = u32::try_from(self.lemmas.len()).map_err(|_| BuildError::TooManyLemmas)?;
                self.lemmas.push(lemma.to_string());
                self.lemma_ids.insert(lemma.to_string(), id);
                id
            }
        };
        let rec = PendingRecord {
            lemma_id,
            pos,
            source,
            features: PackedFeatures::pack(features),
            aux: Aux::to_code(features.aux),
        };
        let bucket = self.by_surface.entry(surface.to_string()).or_default();
        if !bucket.iter().any(|r| {
            r.lemma_id == rec.lemma_id
                && r.pos == rec.pos
                && r.source == rec.source
                && r.features == rec.features
                && r.aux == rec.aux
        }) {
            bucket.push(rec);
            self.total_records += 1;
        }
        Ok(())
    }

    /// Finalise the lexicon. Writes the FST to `fst_out` and the side
    /// table to `side_out`. Both writers must be empty on entry.
    pub fn finish<W1: Write, W2: Write>(
        self,
        mut fst_out: W1,
        mut side_out: W2,
    ) -> Result<BuildStats, BuildError> {
        let LexiconBuilder {
            by_surface,
            lemmas,
            total_records,
            ..
        } = self;

        let num_lemmas = u32::try_from(lemmas.len()).map_err(|_| BuildError::TooManyLemmas)?;

        // ---------------- Side table -----------------------------------------
        // Layout:
        //   header [HEADER_SIZE bytes]
        //   lemma_offsets [(num_lemmas + 1) * 4 bytes]
        //   lemma_bytes
        //   shape_table [num_shapes * SHAPE_ENTRY_SIZE bytes]
        //   analyses [num_analyses * ANALYSIS_RECORD_SIZE bytes]

        // Build the analyses block, interning each record's (pos, source,
        // aux, features) tuple into the shape table and storing only a
        // (lemma_id, shape_id) pair per record. Per-surface byte spans are
        // recorded relative to the start of the analyses block; absolute
        // offsets are resolved once the shape-table size is known.
        let mut analyses_bytes: Vec<u8> = Vec::new();
        let mut shape_ids: HashMap<Shape, u16> = HashMap::new();
        let mut shapes: Vec<Shape> = Vec::new();
        let mut surface_spans: Vec<(String, u32, u64)> = Vec::with_capacity(by_surface.len());
        for (surface, records) in by_surface {
            let count =
                u32::try_from(records.len()).map_err(|_| BuildError::TooManyAnalysesPerSurface)?;
            let offset_in_analyses = analyses_bytes.len() as u64;
            for rec in records {
                let shape = Shape {
                    packed_features: rec.features.0,
                    pos: rec.pos as u8,
                    source: rec.source as u8,
                    aux: rec.aux,
                };
                let shape_id = match shape_ids.get(&shape) {
                    Some(&id) => id,
                    None => {
                        if shapes.len() as u32 > MAX_SHAPE_ID {
                            return Err(BuildError::TooManyShapes);
                        }
                        let id = shapes.len() as u16;
                        shapes.push(shape);
                        shape_ids.insert(shape, id);
                        id
                    }
                };
                let on_disk = AnalysisRecord {
                    lemma_id: rec.lemma_id,
                    shape_id,
                };
                analyses_bytes.extend_from_slice(&on_disk.to_bytes());
            }
            surface_spans.push((surface, count, offset_in_analyses));
        }
        let num_shapes = u32::try_from(shapes.len()).map_err(|_| BuildError::TooManyShapes)?;

        let lemma_offsets_offset = HEADER_SIZE as u64;
        let lemma_offsets_bytes = (num_lemmas as u64 + 1) * 4;
        let lemma_bytes_offset = lemma_offsets_offset + lemma_offsets_bytes;
        let lemma_total_bytes: u64 = lemmas.iter().map(|s| s.len() as u64).sum();
        let shape_table_offset = lemma_bytes_offset + lemma_total_bytes;
        let shape_table_bytes = num_shapes as u64 * SHAPE_ENTRY_SIZE as u64;
        let analyses_offset = shape_table_offset + shape_table_bytes;

        let num_analyses = u32::try_from(analyses_bytes.len() / ANALYSIS_RECORD_SIZE)
            .map_err(|_| BuildError::TooManyAnalyses)?;
        let analyses_end = analyses_offset + analyses_bytes.len() as u64;
        let analyses_end_u32 =
            u32::try_from(analyses_end).map_err(|_| BuildError::SideTableTooLarge)?;
        let lemma_offsets_offset_u32 = lemma_offsets_offset as u32;
        let lemma_bytes_offset_u32 = lemma_bytes_offset as u32;
        let shape_table_offset_u32 =
            u32::try_from(shape_table_offset).map_err(|_| BuildError::SideTableTooLarge)?;
        let analyses_offset_u32 =
            u32::try_from(analyses_offset).map_err(|_| BuildError::SideTableTooLarge)?;

        // Resolve per-surface absolute analysis offsets now that the
        // shape table size is known. `surface_spans` preserves the
        // BTreeMap's lexicographic order, so FST insertion stays sorted.
        let mut fst_entries: Vec<(String, u64)> = Vec::with_capacity(surface_spans.len());
        for (surface, count, offset_in_analyses) in surface_spans {
            let absolute_offset = analyses_offset + offset_in_analyses;
            let absolute_offset_u32 =
                u32::try_from(absolute_offset).map_err(|_| BuildError::SideTableTooLarge)?;
            fst_entries.push((surface, pack_fst_value(count, absolute_offset_u32)));
        }

        // Build the FST.
        let mut fst_builder = MapBuilder::new(&mut fst_out)?;
        for (surface, value) in &fst_entries {
            fst_builder.insert(surface, *value)?;
        }
        fst_builder.finish()?;

        // ---------------- Write the side table ------------------------------
        // Header (64 bytes, all little-endian).
        let mut header = [0u8; HEADER_SIZE];
        header[0..12].copy_from_slice(&MAGIC);
        header[12..14].copy_from_slice(&VERSION_MAJOR.to_le_bytes());
        header[14..16].copy_from_slice(&VERSION_MINOR.to_le_bytes());
        header[16..20].copy_from_slice(&0u32.to_le_bytes()); // flags
        header[20..24].copy_from_slice(&num_lemmas.to_le_bytes());
        header[24..28].copy_from_slice(&num_analyses.to_le_bytes());
        header[28..32].copy_from_slice(&lemma_offsets_offset_u32.to_le_bytes());
        header[32..36].copy_from_slice(&lemma_bytes_offset_u32.to_le_bytes());
        header[36..40].copy_from_slice(&analyses_offset_u32.to_le_bytes());
        header[40..44].copy_from_slice(&analyses_end_u32.to_le_bytes());
        header[44..48].copy_from_slice(&num_shapes.to_le_bytes());
        header[48..52].copy_from_slice(&shape_table_offset_u32.to_le_bytes());
        // bytes 52..64 are reserved (already zeroed).
        side_out.write_all(&header)?;

        // Lemma offsets: (num_lemmas + 1) * u32, each relative to
        // lemma_bytes_offset.
        let mut running: u32 = 0;
        for lemma in &lemmas {
            side_out.write_all(&running.to_le_bytes())?;
            running = running
                .checked_add(lemma.len() as u32)
                .ok_or(BuildError::SideTableTooLarge)?;
        }
        // Sentinel for length of the last lemma.
        side_out.write_all(&running.to_le_bytes())?;

        // Lemma bytes.
        for lemma in &lemmas {
            side_out.write_all(lemma.as_bytes())?;
        }

        // Shape table.
        for shape in &shapes {
            side_out.write_all(&shape.to_bytes())?;
        }

        // Analyses.
        side_out.write_all(&analyses_bytes)?;
        side_out.flush()?;

        Ok(BuildStats {
            num_lemmas: num_lemmas as u64,
            num_analyses: num_analyses as u64,
            num_surfaces: fst_entries.len() as u64,
            total_records,
            side_table_bytes: analyses_end as u64,
        })
    }
}

/// Statistics returned from [`LexiconBuilder::finish`].
#[derive(Debug, Clone, Copy)]
pub struct BuildStats {
    pub num_lemmas: u64,
    pub num_analyses: u64,
    pub num_surfaces: u64,
    pub total_records: u64,
    pub side_table_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{Case, Gender, Number};
    use crate::lexicon::load::Lexicon;

    #[test]
    fn is_clean_surface_keeps_single_tokens_and_rejects_multitoken_and_markup() {
        // Single-token surfaces — incl. word-internal compounds, the
        // single-word zu-infinitive of separable verbs, and the
        // single-bracket punctuation entries.
        for ok in [
            "Tisch",
            "groß",
            "stilllegen",
            "abzutauchen",
            "[",
            "]",
            "{",
            "}",
            "...",
        ] {
            assert!(is_clean_surface(ok), "wrongly rejected {ok:?}");
        }
        // Multi-token surfaces (any space) — not analysable as one token.
        for bad in [
            "zu lieben",
            "so genannte",
            "Sicherheitsrat der Vereinten Nationen",
            "tauche ab",
        ] {
            assert!(
                !is_clean_surface(bad),
                "failed to reject multi-token {bad:?}"
            );
        }
        // Contaminated surfaces from leaked template markup / whitespace.
        for bad in [
            "isometrischer      <!--laut Duden keine Steigerung-->er",
            "<small>(schneibte)</small>",
            "-ige\n<!--",
            "Foo{{x}}",
            "a[[b]]",
            "x\ty",
        ] {
            assert!(!is_clean_surface(bad), "failed to reject {bad:?}");
        }
    }

    #[test]
    fn build_and_load_roundtrip_minimal() {
        let mut b = LexiconBuilder::new();
        b.add(
            "Tisch",
            "Tisch",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Tisch",
            "Tisch",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Sg, Case::Dat),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Tische",
            "Tisch",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Pl, Case::Nom),
            Source::Attested,
        )
        .unwrap();
        b.add(
            "Tischen",
            "Tisch",
            UPOS::NOUN,
            Features::noun_form(Gender::Masc, Number::Pl, Case::Dat),
            Source::Attested,
        )
        .unwrap();

        let mut fst_bytes = Vec::new();
        let mut side_bytes = Vec::new();
        let stats = b.finish(&mut fst_bytes, &mut side_bytes).unwrap();
        assert_eq!(stats.num_lemmas, 1);
        assert_eq!(stats.num_analyses, 4);
        assert_eq!(stats.num_surfaces, 3);

        let lex = Lexicon::from_bytes(fst_bytes, side_bytes).unwrap();
        let analyses = lex.analyze("Tisch");
        assert_eq!(analyses.len(), 2);
        assert!(analyses.iter().all(|a| a.lemma == "Tisch"));

        let datives = lex.analyze("Tischen");
        assert_eq!(datives.len(), 1);
        let a = &datives[0];
        assert_eq!(a.lemma, "Tisch");
        assert_eq!(a.features.case, Some(Case::Dat));
        assert_eq!(a.features.number, Some(Number::Pl));

        // Unknown surface returns empty.
        assert!(lex.analyze("Quitsch").is_empty());
    }
}
