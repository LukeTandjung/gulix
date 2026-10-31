use std::collections::BTreeMap;

use helix_core::visual_offset_from_block;
use helix_view::{
    editor::{Editor, GutterType},
    graphics::Rect,
    gutter::GutterFn,
};
use tern_sdk::{
    ui::{self, Bound, Extent, Node, View},
    Session, Surface,
};
use tui::buffer::{Buffer, Cell};

use helix_term::{
    compositor::{Compositor, Context as InputContext},
    job::Jobs,
    ui::{EditorView, Prompt},
};

use crate::{
    DocumentPreview, FragmentId, FragmentKind, Presentation, RenderedFragment, TerminalLayout,
};

use super::{ansi::AnsiWriter, frontend::Msg, style::Style};

struct PreviewLayout {
    id: FragmentId,
    kind: FragmentKind,
    rendered: RenderedFragment,
    start_row: usize,
    end_row: usize,
    start_line: usize,
    end_line: usize,
}

struct InlineLayout {
    id: FragmentId,
    rendered: RenderedFragment,
    row: usize,
    start_col: usize,
    end_col: usize,
}

/// State shared by all layout and node operations for one frame.
pub struct PreviewFrame<'a> {
    session: &'a mut Session<Msg>,
    surface: Surface,
    buffer: Buffer,
    rows: Vec<String>,
    columns: u32,
    modal_open: bool,
    main_height: usize,
    cell_height_px: f64,
}

impl<'a> PreviewFrame<'a> {
    pub fn new(
        session: &'a mut Session<Msg>,
        surface: Surface,
        editor: &mut Editor,
        input: &mut Compositor,
        jobs: &mut Jobs,
    ) -> anyhow::Result<Self> {
        let area = input.size();
        let mut buffer = Buffer::empty(area);
        input.render(
            area,
            &mut buffer,
            &mut InputContext {
                editor,
                jobs,
                scroll: None,
            },
        );
        let (cursor, cursor_kind) = input.cursor(area, editor);
        editor.cursor_cache.reset();
        if let Some(cursor) = cursor {
            Style::cursor(&mut buffer, cursor, cursor_kind);
        }
        let rows = AnsiWriter::rows(&buffer)?;
        let columns = area.width as u32;
        let modal_open = input.layer_count() > 1
            || editor.autoinfo.is_some()
            || input
                .find::<EditorView>()
                .is_some_and(|view| view.has_completion());
        let prompt_open = input.find::<Prompt>().is_some();
        let main_height = if prompt_open {
            rows.len()
        } else {
            TerminalLayout::new(area.height)
                .map_or(area.height.saturating_sub(2) as usize, |layout| {
                    layout.main_region_rows(prompt_open) as usize
                })
        };
        let cell_height_px = {
            let height = session.caps().cell.unwrap_or_default().h;
            if height > 0.0 {
                height
            } else {
                16.0
            }
        };
        Ok(Self {
            session,
            surface,
            buffer,
            rows,
            columns,
            modal_open,
            main_height,
            cell_height_px,
        })
    }

    pub fn render(
        mut self,
        document: &mut DocumentPreview,
        active: Option<FragmentId>,
        editor: &mut Editor,
    ) -> anyhow::Result<()> {
        let view = editor.tree.get(editor.tree.focus);
        let doc = editor.document(view.doc).unwrap();
        let inner = view.inner_area(doc);
        let view_offset = doc.view_offset(view.id);
        let text = doc.text().slice(..);
        let text_fmt = doc.text_format(inner.width, None);
        let annotations = view.text_annotations(doc, None);
        let visual_position = |position| {
            visual_offset_from_block(text, view_offset.anchor, position, &text_fmt, &annotations).0
        };
        let mut layouts: Vec<PreviewLayout> = document
            .presentations(active)
            .into_iter()
            .flatten()
            .filter_map(|(id, presentation)| {
                if self.modal_open || active == Some(id) {
                    return None;
                }
                let Presentation::Rendered {
                    kind,
                    rendered,
                    range,
                    ..
                } = presentation
                else {
                    return None;
                };
                if kind == FragmentKind::InlineMath {
                    return None;
                }
                Some(PreviewLayout {
                    id,
                    kind,
                    rendered: rendered.clone(),
                    start_row: visual_position(range.start).row,
                    end_row: visual_position(range.end.saturating_sub(1)).row,
                    start_line: text.char_to_line(range.start),
                    end_line: text.char_to_line(range.end.saturating_sub(1)),
                })
            })
            .collect();
        layouts.sort_by_key(|layout| layout.start_row);

        let viewport_start = view.area.y as isize;
        let viewport_end =
            (viewport_start + view.area.height as isize).min(self.main_height as isize);
        let vertical_offset = view_offset.vertical_offset as isize;
        let horizontal_offset = view_offset.horizontal_offset as isize;
        let mut inline_layouts: Vec<InlineLayout> = document
            .presentations(active)
            .into_iter()
            .flatten()
            .filter_map(|(id, presentation)| {
                if self.modal_open || active == Some(id) {
                    return None;
                }
                let Presentation::Rendered {
                    kind: FragmentKind::InlineMath,
                    rendered,
                    range,
                    ..
                } = presentation
                else {
                    return None;
                };
                let start = visual_position(range.start);
                let end = visual_position(range.end);
                if start.row != end.row {
                    return None;
                }
                let row = viewport_start + start.row as isize - vertical_offset;
                let start_col = inner.x as isize + start.col as isize - horizontal_offset;
                let end_col = inner.x as isize + end.col as isize - horizontal_offset;
                if row < viewport_start
                    || row >= viewport_end
                    || start_col < inner.x as isize
                    || end_col > inner.x as isize + inner.width as isize
                {
                    return None;
                }
                Some(InlineLayout {
                    id,
                    rendered: rendered.clone(),
                    row: row as usize,
                    start_col: start_col as usize,
                    end_col: end_col as usize,
                })
            })
            .collect();
        inline_layouts.sort_by_key(|layout| (layout.row, layout.start_col));
        let mut inline_by_row: BTreeMap<usize, Vec<InlineLayout>> = BTreeMap::new();
        for layout in inline_layouts {
            inline_by_row.entry(layout.row).or_default().push(layout);
        }
        let mut inline_nodes = BTreeMap::new();
        for (row, layouts) in inline_by_row {
            inline_nodes.insert(
                row,
                Self::inline_row(
                    self.session,
                    &self.buffer,
                    row,
                    &layouts,
                    self.cell_height_px,
                )?,
            );
        }

        let mut children = Vec::new();
        let mut line_numbers =
            GutterType::LineNumbers.style(editor, doc, view, &editor.theme, true);
        let linenr_trailing = view
            .gutters
            .layout
            .iter()
            .rev()
            .take_while(|gutter| !matches!(gutter, GutterType::LineNumbers))
            .count() as u16;
        let row_budget_px = self.main_height as f64 * self.cell_height_px;
        let mut consumed_px = 0.0;
        let mut row = 0;
        for layout in layouts {
            let start = viewport_start + layout.start_row as isize - vertical_offset;
            let end = viewport_start + layout.end_row as isize - vertical_offset;
            if end < viewport_start || start >= viewport_end {
                continue;
            }
            let replace_start = start.max(viewport_start).max(0) as usize;
            let replace_end = end.min(viewport_end - 1) as usize;
            if replace_start < row || replace_start > replace_end {
                continue;
            }
            if row < replace_start {
                self.push_rows(&mut children, row, replace_start, &inline_nodes);
                consumed_px += (replace_start - row) as f64 * self.cell_height_px;
            }
            let preview_height_px = layout.rendered.intrinsic_height_px.ceil().max(1.0) as f64;
            children.push(self.preview_node(
                &layout,
                inner.x,
                &mut line_numbers,
                linenr_trailing,
                inner.width,
            )?);
            consumed_px += preview_height_px;
            row = replace_end + 1;
        }
        if row < self.main_height && consumed_px < row_budget_px {
            let remaining_rows =
                ((row_budget_px - consumed_px) / self.cell_height_px).floor() as usize;
            let fill_end = row.saturating_add(remaining_rows).min(self.main_height);
            if row < fill_end {
                self.push_rows(&mut children, row, fill_end, &inline_nodes);
            }
        }

        self.session.render(
            self.surface,
            View::new()
                .main(
                    ui::col().child(
                        ui::col()
                            .gap(ui::Gap::None)
                            .min(Bound::h(Extent::Fraction(1.0)))
                            .grow(1.0)
                            .children(children),
                    ),
                )
                .layer(
                    ui::col().child(
                        ui::col()
                            .role("gulix-status")
                            .min(Bound::h(Extent::Fraction(1.0)))
                            .justify(ui::Justify::End)
                            .children(
                                self.rows[self.main_height..]
                                    .iter()
                                    .map(|row| Node::from(ui::ansi(row.clone()).cols(self.columns)))
                                    .chain(std::iter::once(Node::from(
                                        ui::spacer().min(Bound::h(Extent::Lines(1.0))),
                                    ))),
                            ),
                    ),
                ),
        )?;
        Ok(())
    }

    fn push_rows(
        &self,
        children: &mut Vec<Node<Msg>>,
        start: usize,
        end: usize,
        inline_nodes: &BTreeMap<usize, Node<Msg>>,
    ) {
        for (row, source) in self.rows.iter().enumerate().skip(start).take(end - start) {
            if let Some(node) = inline_nodes.get(&row) {
                children.push(node.clone());
            } else {
                children.push(
                    ui::rows(vec![source.clone()])
                        .cols(self.columns)
                        .on_click(Msg::Deactivate)
                        .into(),
                );
            }
        }
    }

    fn inline_row(
        session: &mut Session<Msg>,
        buffer: &Buffer,
        row: usize,
        layouts: &[InlineLayout],
        cell_height_px: f64,
    ) -> anyhow::Result<Node<Msg>> {
        let width = buffer.area.width as usize;
        let cells = &buffer.content()[row * width..(row + 1) * width];
        let mut children = Vec::new();
        let mut column = 0;
        for layout in layouts {
            if column < layout.start_col {
                children.push(Self::ansi_segment(
                    &cells[column..layout.start_col],
                    layout.start_col - column,
                )?);
            }
            let blob = session.blob(
                layout.rendered.svg.clone().into_bytes(),
                Some("image/svg+xml"),
            )?;
            let width_px = f64::from(layout.rendered.intrinsic_width_px).max(1.0);
            children.push(
                ui::image()
                    .key(format!("fragment-{}", layout.id.0))
                    .blob(blob)
                    .w(width_px)
                    .h(cell_height_px.ceil())
                    .min(Bound::h(Extent::Lines(1.0)))
                    .max(Bound::h(Extent::Lines(1.0)))
                    .grow(0.0)
                    .shrink(0.0)
                    .basis(ui::Basis::Content)
                    .alt("Inline math. Click to edit its source.")
                    .on_click(Msg::Reveal(layout.id))
                    .into(),
            );
            column = layout.end_col;
        }
        if column < width {
            children.push(Self::ansi_segment(&cells[column..], width - column)?);
        }
        Ok(ui::row()
            .gap(ui::Gap::None)
            .align(ui::Align::Baseline)
            .children(children)
            .into())
    }

    fn ansi_segment(cells: &[Cell], columns: usize) -> anyhow::Result<Node<Msg>> {
        Ok(ui::rows(vec![AnsiWriter::row(cells)?])
            .role("gulix-inline-text")
            .cols(columns as u32)
            .min(Bound::w(Extent::Ch(columns as f64)))
            .max(Bound::w(Extent::Ch(columns as f64)))
            .grow(0.0)
            .shrink(0.0)
            .basis(ui::Basis::Content)
            .on_click(Msg::Deactivate)
            .into())
    }

    fn line_number_node(
        line: usize,
        gutter_width: u16,
        line_numbers: &mut GutterFn<'_>,
        linenr_trailing: u16,
    ) -> anyhow::Result<Node<Msg>> {
        let mut label = String::new();
        let style = line_numbers(line, false, true, &mut label).unwrap_or_default();
        let mut buffer = Buffer::empty(Rect::new(0, 0, gutter_width.max(1), 1));
        let label_width = label.chars().count() as u16;
        let x = TerminalLayout::line_number_x(gutter_width, linenr_trailing, label_width);

        buffer.set_string(x, 0, &label, style);

        Ok(ui::ansi(AnsiWriter::row(buffer.content())?)
            .cols(gutter_width as u32)
            .min(Bound::wh(
                Extent::Ch(gutter_width as f64),
                Extent::Lines(1.0),
            ))
            .max(Bound::wh(
                Extent::Ch(gutter_width as f64),
                Extent::Lines(1.0),
            ))
            .grow(0.0)
            .shrink(0.0)
            .into())
    }

    fn preview_node(
        &mut self,
        layout: &PreviewLayout,
        inner_x: u16,
        line_numbers: &mut GutterFn<'_>,
        linenr_trailing: u16,
        inner_width: u16,
    ) -> anyhow::Result<Node<Msg>> {
        let PreviewLayout {
            id,
            kind,
            rendered,
            start_line,
            end_line,
            ..
        } = layout;
        let blob = self
            .session
            .blob(rendered.svg.clone().into_bytes(), Some("image/svg+xml"))?;
        let height_px = f64::from(rendered.intrinsic_height_px).ceil().max(1.0);
        let image: Node<Msg> = ui::image()
            .key(format!("fragment-{}", id.0))
            .blob(blob)
            .w(f64::from(rendered.intrinsic_width_px).ceil().max(1.0))
            .h(height_px)
            .grow(0.0)
            .shrink(0.0)
            .basis(ui::Basis::Content)
            .alt("Typst block. Click to edit its source.")
            .on_click(Msg::Reveal(*id))
            .into();
        let mut gutter = ui::col()
            .min(Bound::wh(
                Extent::Ch(inner_x as f64),
                Extent::Lines(height_px / self.cell_height_px),
            ))
            .max(Bound::w(Extent::Ch(inner_x as f64)))
            .justify(ui::Justify::Between)
            .grow(0.0)
            .shrink(0.0)
            .child(Self::line_number_node(
                *start_line,
                inner_x,
                line_numbers,
                linenr_trailing,
            )?);
        if end_line != start_line {
            gutter = gutter.child(Self::line_number_node(
                *end_line,
                inner_x,
                line_numbers,
                linenr_trailing,
            )?);
        }
        if matches!(kind, FragmentKind::DisplayMath | FragmentKind::Figure) {
            let column = ui::col()
                .align(ui::Align::Center)
                .min(Bound::w(Extent::Ch(inner_width as f64)))
                .child(image);
            Ok(ui::row().child(gutter).child(column).into())
        } else {
            Ok(ui::row().child(gutter).child(image).into())
        }
    }
}
