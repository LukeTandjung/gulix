//! Pure chrome geometry mirroring Helix's own terminal layout constants.
//!
//! Helix embeds these rules in effectful editor and overlay rendering. Sharing
//! them would first require a broad UI refactor to separate pure layout policy
//! from rendering effects. Until that boundary exists, this module deliberately
//! keeps a small compatibility model rather than coupling core to Helix's UI.
//!
//! Naming the policy explicitly also leaves a path towards editor-agnostic
//! frontend contracts. Helix can remain one implementation of these rules while
//! future editor integrations share the pure policy without inheriting its UI.

use vstd::prelude::*;

verus! {

/// Rows `ui::editor::EditorView::render` reserves at the bottom of its area:
/// the status line and the command/pending-key line.
pub const HELIX_RESERVED_ROWS: u16 = 2;

/// Rows `ui::overlay::overlaid` clears below centered dialogs.
pub const HELIX_OVERLAY_CLEARANCE: u16 = 2;

/// Where Helix's chrome sits inside a terminal area, matching plain Helix 1:1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalLayout {
    /// Rows above the chrome; editor content and dialogs live here.
    pub main_rows: u16,
    /// The status line's row: second-to-last, as in `ui::statusline::render`.
    pub status_row: u16,
    /// The command/pending-key line's row: last, as in `EditorView::render`.
    pub command_row: u16,
    /// The last row a centered dialog (`ui::overlay::overlaid`) may occupy.
    pub dialog_last_row: u16,
}

impl TerminalLayout {
    /// Mirrors Helix's placement inside an area of `terminal_rows` rows:
    /// `ui::statusline::render` pins the status line to the second-to-last row,
    /// `EditorView::render` writes pending keys on the last row, and
    /// `ui::overlay::overlaid` keeps dialogs clear of both. Returns `None` when
    /// the area is too short to hold the chrome at all.
    pub fn new(terminal_rows: u16) -> (result: Option<TerminalLayout>)
        ensures
            (terminal_rows >= HELIX_RESERVED_ROWS + 1) == (result is Some),
            result is Some
                ==> result->Some_0.command_row + 1 == terminal_rows
                    && result->Some_0.status_row + 2 == terminal_rows
                    && result->Some_0.main_rows == result->Some_0.status_row
                    && result->Some_0.dialog_last_row + HELIX_OVERLAY_CLEARANCE + 1 == terminal_rows
                    && result->Some_0.dialog_last_row < result->Some_0.status_row,
    {
        if terminal_rows < HELIX_RESERVED_ROWS + 1 {
            None
        } else {
            let command_row = terminal_rows - 1;
            let status_row = terminal_rows - HELIX_RESERVED_ROWS;
            let dialog_last_row = terminal_rows - HELIX_OVERLAY_CLEARANCE - 1;
            Some(TerminalLayout {
                main_rows: status_row,
                status_row,
                command_row,
                dialog_last_row,
            })
        }
    }

    /// Rows that belong to the continuous main region. Prompt completions may
    /// cover the status-line row (`ui::prompt::Prompt::render_prompt`), so an open
    /// prompt keeps all rows together instead of splitting them around the chrome
    /// spacer. Other UI uses the normal Helix content/dialog boundary.
    pub fn main_region_rows(self, prompt_open: bool) -> (result: u16)
        requires
            self.command_row < u16::MAX,
        ensures
            prompt_open ==> result == self.command_row + 1,
            !prompt_open ==> result == self.main_rows,
    {
        if prompt_open {
            self.command_row + 1
        } else {
            self.main_rows
        }
    }
}

} // verus!
