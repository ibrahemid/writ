//! Keybinding conflict reporting.

/// A conflict between a user-defined keybinding and an editor-level
/// CodeMirror command.
///
/// Writ reports conflicts so the host can warn the user that a shortcut
/// they configured will be shadowed by a built-in editor action.
pub struct KeybindingConflict {
    /// Writ command the keybinding is bound to.
    pub command_id: String,
    /// The user-configured keybinding string.
    pub keybinding: String,
    /// Editor-level command the keybinding shadows.
    pub shadowed_editor_command: String,
}
