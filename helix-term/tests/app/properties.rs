use std::collections::HashSet;

use helix_core::Rope;
use helix_term::typst_preview::{DocumentPreview, FragmentId, Presentation};
use quickcheck::{quickcheck, Arbitrary, Gen, TestResult};

use super::support::{context, ids};

#[derive(Clone, Copy, Debug)]
enum BlockKind {
    InlineMath,
    DisplayMath,
    CodeBlock,
    Table,
    Image,
    Figure,
}

impl Arbitrary for BlockKind {
    fn arbitrary(generator: &mut Gen) -> Self {
        match u8::arbitrary(generator) % 6 {
            0 => Self::InlineMath,
            1 => Self::DisplayMath,
            2 => Self::CodeBlock,
            3 => Self::Table,
            4 => Self::Image,
            _ => Self::Figure,
        }
    }
}

impl BlockKind {
    fn source(self, index: usize) -> String {
        match self {
            Self::InlineMath => format!("$x_{index}$"),
            Self::DisplayMath => format!("$ x_{index} + y $"),
            Self::CodeBlock => format!("```text\nvalue {index}\n```"),
            Self::Table => format!("#table(columns: 1, [cell {index}])"),
            Self::Image => format!("#image(\"figure-{index}.svg\")"),
            Self::Figure => {
                format!("#figure(image(\"figure-{index}.svg\"), caption: [Figure {index}])")
            }
        }
    }
}

quickcheck! {
    fn generated_documents_have_valid_ranges(blocks: Vec<BlockKind>) -> TestResult {
        if blocks.is_empty() || blocks.len() > 12 {
            return TestResult::discard();
        }
        let expected = sources(&blocks);
        TestResult::from_bool(valid_document(&mut document(&expected), &expected))
    }

    fn insertions_preserve_unchanged_ids_and_ranges(blocks: Vec<BlockKind>, insertion: usize) -> TestResult {
        if blocks.is_empty() || blocks.len() > 12 {
            return TestResult::discard();
        }
        let insertion = insertion % (blocks.len() + 1);
        let original_sources = sources(&blocks);
        let mut document = document(&original_sources);
        let original_ids = ids(&mut document);
        let mut edited = original_sources.clone();
        edited.insert(insertion, format!("$ inserted_{insertion} $"));
        if document.replace_source(Rope::from(join(&edited))).is_err() {
            return TestResult::failed();
        }
        let edited_ids = ids(&mut document);
        let ids_preserved = original_ids.iter().enumerate().all(|(index, id)| {
            let edited_index = if index >= insertion { index + 1 } else { index };
            edited_ids.get(edited_index) == Some(id)
        });
        TestResult::from_bool(
            ids_preserved
                && unique_ids(&edited_ids)
                && valid_document(&mut document, &edited),
        )
    }
}

fn sources(blocks: &[BlockKind]) -> Vec<String> {
    blocks
        .iter()
        .enumerate()
        .map(|(index, block)| block.source(index))
        .collect()
}

fn document(sources: &[String]) -> DocumentPreview {
    DocumentPreview::new(Rope::from(join(sources)), context()).expect("generated valid document")
}

fn join(sources: &[String]) -> String {
    sources.join("\n\n")
}

fn unique_ids(ids: &[FragmentId]) -> bool {
    let unique: HashSet<_> = ids.iter().copied().collect();
    unique.len() == ids.len()
}

fn valid_document(document: &mut DocumentPreview, expected: &[String]) -> bool {
    let source = join(expected);
    let unique_ids = unique_ids(&ids(document));
    let presentations: Vec<_> = document
        .presentations(None)
        .expect("presentation")
        .collect();
    if presentations.len() != expected.len() || !unique_ids {
        return false;
    }
    let mut previous_end = 0;
    for (index, (_, presentation)) in presentations.into_iter().enumerate() {
        let Presentation::Source { range, source: raw } = presentation else {
            return false;
        };
        if range.start < previous_end
            || range.end > source.chars().count()
            || raw.to_string() != expected[index]
        {
            return false;
        }
        previous_end = range.end;
    }
    true
}
