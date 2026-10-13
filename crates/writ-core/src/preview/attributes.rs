//! The attribute list of an HTML start tag, read one byte at a time.
//!
//! The states and transitions are the HTML Living Standard's (13.2.5.32 to
//! 13.2.5.40), from the byte after a tag's name to the `>` that ends the tag.
//! Each step consumes one byte; a transition the standard makes by reconsuming
//! a byte in another state is taken within the same step. A carriage return
//! reaches the tokenizer as a line feed, and neither a NUL nor a character
//! reference moves a boundary, so the bytes alone decide every transition.

/// HTML's whitespace as the tokenizer sees it. A carriage return reaches the
/// tokenizer as a line feed.
pub(super) fn is_html_space(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | b'\x0C' | b'\r' | b' ')
}

/// Where `<` at `open` names the tag `name` (ASCII case-insensitive) followed
/// by a byte that ends a tag name, the position of that byte: the attribute
/// list starts there.
pub(super) fn tag_name_end(bytes: &[u8], open: usize, name: &[u8]) -> Option<usize> {
    let end = open + 1 + name.len();
    let candidate = bytes.get(open + 1..end)?;
    let terminator = *bytes.get(end)?;
    (bytes[open] == b'<'
        && candidate.eq_ignore_ascii_case(name)
        && (is_html_space(terminator) || terminator == b'/' || terminator == b'>'))
        .then_some(end)
}

/// The tokenizer states an attribute list passes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
    BeforeName,
    Name,
    AfterName,
    BeforeValue,
    Quoted(u8),
    Unquoted,
    AfterQuoted,
    SelfClosing,
}

/// What one byte does to an attribute list.
#[derive(Clone, Copy, Debug)]
pub(super) struct Step {
    /// The state after the byte, or `None` when the byte ends the tag.
    pub(super) next: Option<State>,
    /// The byte is the first of an attribute name.
    pub(super) name_starts: bool,
    /// An attribute name ended just before the byte.
    pub(super) name_ends: bool,
    /// An attribute value starts at the byte (`Some(0)`, unquoted) or just
    /// after it (`Some(1)`, the byte is an opening quote).
    pub(super) value_starts: Option<usize>,
    /// An attribute value ended just before the byte.
    pub(super) value_ends: bool,
}

/// Read one byte of an attribute list in `state`.
pub(super) fn advance(state: State, byte: u8) -> Step {
    let mut step = Step {
        next: Some(state),
        name_starts: false,
        name_ends: false,
        value_starts: None,
        value_ends: false,
    };
    let space = is_html_space(byte);
    match state {
        State::BeforeName => before_name(&mut step, byte),
        State::Name => {
            if space || byte == b'/' || byte == b'>' || byte == b'=' {
                step.name_ends = true;
                if byte == b'=' {
                    step.next = Some(State::BeforeValue);
                } else {
                    after_name(&mut step, byte);
                }
            }
        }
        State::AfterName => after_name(&mut step, byte),
        State::BeforeValue => {
            if byte == b'"' || byte == b'\'' {
                step.next = Some(State::Quoted(byte));
                step.value_starts = Some(1);
            } else if byte == b'>' {
                // A missing value is an empty one, and the tag ends here.
                step.next = None;
            } else if !space {
                step.next = Some(State::Unquoted);
                step.value_starts = Some(0);
            }
        }
        State::Quoted(quote) => {
            if byte == quote {
                step.value_ends = true;
                step.next = Some(State::AfterQuoted);
            }
        }
        State::Unquoted => {
            if space || byte == b'>' {
                step.value_ends = true;
                step.next = (byte != b'>').then_some(State::BeforeName);
            }
        }
        State::AfterQuoted => {
            if space {
                step.next = Some(State::BeforeName);
            } else if byte == b'/' {
                step.next = Some(State::SelfClosing);
            } else if byte == b'>' {
                step.next = None;
            } else {
                before_name(&mut step, byte);
            }
        }
        State::SelfClosing => {
            if byte == b'>' {
                step.next = None;
            } else {
                before_name(&mut step, byte);
            }
        }
    }
    step
}

/// The before-attribute-name state on one byte.
fn before_name(step: &mut Step, byte: u8) {
    if is_html_space(byte) {
        step.next = Some(State::BeforeName);
    } else if byte == b'/' || byte == b'>' {
        after_name(step, byte);
    } else {
        // `=` here starts a name of its own, as any other byte does.
        step.next = Some(State::Name);
        step.name_starts = true;
    }
}

/// The after-attribute-name state on one byte.
fn after_name(step: &mut Step, byte: u8) {
    if is_html_space(byte) {
        step.next = Some(State::AfterName);
    } else if byte == b'/' {
        step.next = Some(State::SelfClosing);
    } else if byte == b'=' {
        step.next = Some(State::BeforeValue);
    } else if byte == b'>' {
        step.next = None;
    } else {
        step.next = Some(State::Name);
        step.name_starts = true;
    }
}
