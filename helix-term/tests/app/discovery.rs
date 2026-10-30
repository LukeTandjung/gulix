use std::collections::HashSet;

use helix_core::Rope;
use helix_term::typst_preview::Presentation;
use quickcheck::{quickcheck, Arbitrary, Gen, TestResult};

use super::support::{document, ids};

#[derive(Clone, Debug)]
enum InputBlock {
    InlineMath(u8),
    DisplayMath(u8),
    CodeBlock(u8),
    Table(u8),
    Image(u8),
    Figure(u8),
    Prose(u8),
    Comment(u8),
    Raw(u8),
    IncompleteMath(u8),
}

impl Arbitrary for InputBlock {
    fn arbitrary(generator: &mut Gen) -> Self {
        let value = u8::arbitrary(generator);
        match value % 10 {
            0 => Self::InlineMath(value),
            1 => Self::DisplayMath(value),
            2 => Self::CodeBlock(value),
            3 => Self::Table(value),
            4 => Self::Image(value),
            5 => Self::Figure(value),
            6 => Self::Prose(value),
            7 => Self::Comment(value),
            8 => Self::Raw(value),
            _ => Self::IncompleteMath(value),
        }
    }
}

impl InputBlock {
    fn source(&self) -> String {
        match self {
            Self::InlineMath(value) => format!("$x_{value}$"),
            Self::DisplayMath(value) => format!("$ x_{value} + y $"),
            Self::CodeBlock(value) => format!("```text\nvalue {value}\n```"),
            Self::Table(value) => format!("#table(columns: 1, [cell {value}])"),
            Self::Image(value) => format!("#image(\"figure-{value}.svg\")"),
            Self::Figure(value) => {
                format!("#figure(image(\"figure-{value}.svg\"), caption: [Figure {value}])")
            }
            Self::Prose(value) => format!("= Section {value}\n\nOrdinary prose {value}"),
            Self::Comment(value) => format!("// $ hidden_{value} $"),
            Self::Raw(value) => format!("```typ\n$ hidden_{value} $\n```"),
            Self::IncompleteMath(value) => format!("$ unfinished_{value}"),
        }
    }

    fn fragment(&self) -> Option<String> {
        match self {
            Self::Prose(_) | Self::Comment(_) | Self::IncompleteMath(_) => None,
            _ => Some(self.source()),
        }
    }
}

fn unicode(bytes: &[u8]) -> String {
    const VALUES: &[&str] = &["λ", "😀", "é", "界", "a"];
    bytes
        .iter()
        .map(|byte| VALUES[*byte as usize % VALUES.len()])
        .collect()
}

quickcheck! {
    fn generated_markup_discovers_exactly_the_supported_fragments(blocks: Vec<InputBlock>) -> TestResult {
        let source = blocks
            .iter()
            .enumerate()
            .filter(|(index, block)| {
                *index + 1 == blocks.len() || !matches!(block, InputBlock::IncompleteMath(_))
            })
            .map(|(_, block)| block.source())
            .collect::<Vec<_>>()
            .join("\n\n");
        let expected: Vec<_> = blocks.iter().filter_map(InputBlock::fragment).collect();
        let mut document = document(&source);
        let ids: HashSet<_> = ids(&mut document).into_iter().collect();
        let presentations: Vec<_> = document.presentations(None).expect("presentation").collect();
        if presentations.len() != expected.len() {
            return TestResult::failed();
        }
        let mut previous_end = 0;

        for (index, (_, presentation)) in presentations.into_iter().enumerate() {
            let Presentation::Source { range, source: raw, .. } = presentation else {
                return TestResult::failed();
            };
            if raw.to_string() != expected[index]
                || range.start < previous_end
                || range.end > source.chars().count()
            {
                return TestResult::failed();
            }
            previous_end = range.end;
        }

        TestResult::from_bool(ids.len() == expected.len())
    }

    fn unicode_insertions_preserve_fragment_ids_and_character_ranges(
        prefix: Vec<u8>,
        insertion: Vec<u8>,
        value: u8
    ) -> TestResult {
        let prefix = unicode(&prefix);
        let insertion = unicode(&insertion);
        let fragment = format!("$ x_{value} $");
        let mut document = document(&format!("{prefix}\n\n{fragment}"));
        let original_id = ids(&mut document)[0];
        document
            .replace_source(Rope::from(format!("{prefix}{insertion}\n\n{fragment}")))
            .expect("Unicode replacement must be valid");
        let edited_ids = ids(&mut document);
        let Some((_, Presentation::Source { range, source })) = document.presentations(None).and_then(|mut presentations| presentations.next()) else {
            return TestResult::failed();
        };

        TestResult::from_bool(
            edited_ids == vec![original_id]
                && range.start == prefix.chars().count() + insertion.chars().count() + 2
                && source.to_string() == fragment,
        )
    }

    fn nearest_same_kind_edits_preserve_identity(
        first: u8,
        second: u8,
        changed: u8,
        edit_second: bool
    ) -> TestResult {
        let mut source = vec![format!("$ x_{first} $"), format!("$ x_{second} $")];
        let mut document = document(&source.join("\n\n"));
        let original_ids = ids(&mut document);
        source[usize::from(edit_second)] = format!("$ changed_{changed} $");
        document
            .replace_source(Rope::from(source.join("\n\n")))
            .expect("same-kind replacement must be valid");

        TestResult::from_bool(ids(&mut document) == original_ids)
    }
}
