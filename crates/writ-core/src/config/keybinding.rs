pub struct KeybindingConflict {
    pub command_id: String,
    pub keybinding: String,
    pub shadowed_editor_command: String,
}
