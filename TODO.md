# Gulix Typst TODO

Render a small set of everyday Typst blocks inside the normal Helix editor UI.
The source buffer remains authoritative, and no separate PDF or preview pane is
needed.

## Requirements

- Keep the full Helix compositor, including ordinary text, the gutter, cursor,
  selections, status line, diagnostics, and normal keyboard editing. Use the
  pty's real dimensions, keep overlays bottom-aligned with the raised status
  area, and preserve Tern's normal horizontal terminal inset.
- Render only these block constructs:
  - Inline math (`$...$`) and display math (`$ ... $`), including the
    symbols, fractions, scripts, sums, vectors, and other constructs from the
    tutorial.
  - Fenced code blocks with optional language tags and syntax highlighting.
  - Basic `#table(...)` calls with columns, rows, cells, and header cells.
  - Basic local images and simple figures with captions.
- Keep paragraphs, headings, emphasis, lists, and all other ordinary prose as
  normal Helix text; do not turn them into SVG previews.
- Preserve Typst's default formatting instead of making every block a uniform,
  full-width panel:
  - Treat the surrounding body text size as `1em`; do not impose Typst's 11pt
    default as a fixed terminal cell size.
  - Inline equations use a suitable math font at the surrounding nominal size
    and share the text baseline instead of becoming oversized images. Display
    equations use their intrinsic size and are centred within the text column.
  - Raw code uses a monospace font at `0.8em`, start-aligned lines, preserved
    whitespace, and tab stops of two spaces.
  - Tables size automatic tracks to their content, use `5pt` cell inset and
    `1pt` cell strokes, and inherit the surrounding alignment. Structural table
    strokes are content, not panel chrome.
  - Plain images are block-level with natural width and height unless the
    source overrides them, and remain start-aligned. Figures are centred,
    with captions below and a `0.65em` body-to-caption gap.
  - Content keeps its intrinsic width by default. Only explicit
    fractional/percentage sizing or the source's own layout rules should use
    the available measure; do not stretch every block to the full pty width.
- Keep source text unchanged and map every rendered block back to its source
  range, including Helix's soft-wrap and viewport coordinates.
- Render blocks in place, not as pages or as a separate document preview.
  Show the raw source's first and last line numbers beside each block preview;
  a single-line block shows its line number once.
- Make rendered blocks seamless:
  - No cards, borders, shadows, rounded frames, mode badges, buttons, or render
    controls around blocks. Typst's own structural styling, such as table cell
    strokes, is preserved.
  - Use the editor's background, foreground, theme, and available width.
  - Use transparent SVGs and natural document spacing.
  - Show only the caret and text selection needed while editing.
- Start with no active preview block while retaining the normal Helix cursor
  and status line.
- Allow zero or one active preview blocks, never more than one.
- Clicking a rendered block reveals its raw source in the same place.
- Clicking outside the active block recompiles and renders it again, including
  clicking another block or leaving the Gulix pane.
- Keep a block raw after edits until its latest source compiles.
- Discard stale compilation results. Leave diagnostics to the existing Typst
  LSP and Helix's diagnostic rendering; do not add Tern-specific error UI.
- Run parsing and compilation outside Tern's render callback, asynchronously,
  with revision and width/theme-aware caching.
- Preserve correct cursor movement, scrolling, and source ranges when edits or
  rendering change block heights.
