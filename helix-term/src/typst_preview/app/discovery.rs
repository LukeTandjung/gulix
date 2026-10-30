//! Private syntax discovery and stable-identity reconciliation.

use std::{collections::HashSet, ops::Range};

use typst_syntax::{ast, SyntaxKind, SyntaxNode};

use crate::typst_preview::core::{Fragment, FragmentId, FragmentKind};

/// Parser output has no identity yet; reconciliation supplies stable IDs separately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredFragment {
    pub range: Range<usize>,
    pub kind: FragmentKind,
    pub source: String,
}

/// Discovers flat display segments and skips incomplete syntax without producing diagnostics.
pub fn discover_fragments(source: &str) -> Vec<DiscoveredFragment> {
    let root = typst_syntax::parse(source);
    let mut fragments = Vec::new();

    visit(&root, source, 0, 0, &mut fragments);
    fragments
}

fn visit(
    node: &SyntaxNode,
    source: &str,
    node_start: usize,
    range_start: usize,
    fragments: &mut Vec<DiscoveredFragment>,
) {
    if node.kind() == SyntaxKind::Markup {
        let mut children = node.children().scan(node_start, |offset, child| {
            let start = *offset;
            *offset += child.len();
            Some((start, child))
        });

        while let Some((start, child)) = children.next() {
            match child.kind() {
                SyntaxKind::Hash => {
                    while let Some((child_start, child)) = children.next() {
                        if child.kind() == SyntaxKind::Space {
                            continue;
                        }

                        if let Some(fragment) = classify(child, source, child_start, start) {
                            fragments.push(fragment);
                        }

                        break;
                    }
                }
                SyntaxKind::LineComment | SyntaxKind::BlockComment => {}
                _ => {
                    if let Some(fragment) = classify(child, source, start, start) {
                        fragments.push(fragment);
                    } else {
                        visit(child, source, start, start, fragments);
                    }
                }
            }
        }

        return;
    }

    // An error in one construct must not hide unrelated constructs elsewhere
    // in the document. Skip the erroneous preview itself, but keep walking
    // error-bearing ancestors so their other children retain stable IDs.
    if node.diagnosis().errors
        && matches!(
            node.kind(),
            SyntaxKind::Equation | SyntaxKind::Raw | SyntaxKind::FuncCall
        )
    {
        return;
    }

    if let Some(fragment) = classify(node, source, node_start, range_start) {
        fragments.push(fragment);
        return;
    }

    // Typst syntax lengths are byte offsets; `classify` converts them to character ranges.
    for (start, child) in node.children().scan(node_start, |offset, child| {
        let start = *offset;
        *offset += child.len();
        Some((start, child))
    }) {
        visit(child, source, start, start, fragments);
    }
}

fn classify(
    node: &SyntaxNode,
    source: &str,
    node_start: usize,
    range_start: usize,
) -> Option<DiscoveredFragment> {
    use ast::AstNode;

    if node.diagnosis().errors {
        return None;
    }

    let kind = match node.kind() {
        SyntaxKind::Equation => {
            if ast::Equation::from_untyped(node)?.block() {
                FragmentKind::DisplayMath
            } else {
                FragmentKind::InlineMath
            }
        }
        SyntaxKind::Raw => {
            if ast::Raw::from_untyped(node)?.block() {
                FragmentKind::CodeBlock
            } else {
                return None;
            }
        }
        SyntaxKind::FuncCall => {
            // Markup calls include their leading # in the revealed source range.
            if range_start == node_start {
                return None;
            }

            let call = ast::FuncCall::from_untyped(node)?;
            let ast::Expr::Ident(identifier) = call.callee() else {
                return None;
            };

            match identifier.as_str() {
                "table" => FragmentKind::Table,
                "image" => FragmentKind::Image,
                "figure" => FragmentKind::Figure,
                _ => return None,
            }
        }
        _ => return None,
    };

    let end = node_start + node.len();
    let fragment_source = source.get(range_start..end)?;
    let start_char = source[..range_start].chars().count();
    let end_char = start_char + fragment_source.chars().count();
    if start_char == end_char {
        return None;
    }

    Some(DiscoveredFragment {
        range: start_char..end_char,
        kind,
        source: fragment_source.to_owned(),
    })
}

/// Exact content matches survive moves; nearest same-kind matches survive local edits.
pub fn reconcile_fragments<'a>(
    discovered: Vec<DiscoveredFragment>,
    previous: impl IntoIterator<Item = &'a Fragment>,
    previous_source: &str,
) -> Vec<Fragment> {
    let previous: Vec<(&Fragment, String)> = previous
        .into_iter()
        .map(|fragment| {
            let source = previous_source
                .chars()
                .skip(fragment.range.start)
                .take(fragment.range.end - fragment.range.start)
                .collect();
            (fragment, source)
        })
        .collect();

    let mut used = HashSet::new();
    let mut assignments = vec![None; discovered.len()];

    // Reserve exact matches globally before any nearest-match fallback can steal an ID.
    for exact_only in [true, false] {
        for (index, fragment) in discovered.iter().enumerate() {
            if assignments[index].is_some() {
                continue;
            }

            let previous_index = {
                let mut best: Option<(usize, (usize, usize))> = None;
                for (index, (previous_fragment, old_source)) in previous.iter().enumerate() {
                    if previous_fragment.kind != fragment.kind
                        || used.contains(&previous_fragment.id)
                        || (exact_only && old_source != &fragment.source)
                    {
                        continue;
                    }

                    let score = (
                        previous_fragment.range.start.abs_diff(fragment.range.start),
                        index,
                    );
                    if best
                        .as_ref()
                        .is_none_or(|(_, best_score)| score < *best_score)
                    {
                        best = Some((index, score));
                    }
                }

                best.map(|(index, _)| index)
            };

            if let Some(previous_index) = previous_index {
                let id = previous[previous_index].0.id;
                used.insert(id);
                assignments[index] = Some(id);
            }
        }
    }

    let mut next_id = previous
        .iter()
        .map(|(fragment, _)| fragment.id.0)
        .max()
        .map_or(0, |id| id.wrapping_add(1));
    discovered
        .into_iter()
        .enumerate()
        .map(|(index, discovered)| {
            let id = assignments[index].unwrap_or_else(|| {
                let id = FragmentId(next_id);
                next_id = next_id.wrapping_add(1);
                id
            });
            Fragment {
                id,
                range: discovered.range,
                kind: discovered.kind,
            }
        })
        .collect()
}
