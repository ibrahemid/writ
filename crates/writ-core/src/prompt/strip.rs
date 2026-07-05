/// Produces the paste-ready form of a prompt document.
///
/// Strips leading YAML frontmatter (first line exactly `---`, closed by a
/// later `---` line; unterminated frontmatter is preserved as content) and
/// HTML comments outside fenced code blocks (comments inside ``` or ~~~
/// fences are preserved; an unterminated `<!--` is preserved). Trailing
/// whitespace is trimmed per line and the result ends with exactly one
/// final newline; whitespace-only results collapse to the empty string.
pub fn strip_for_prompt(input: &str) -> String {
    let body = strip_frontmatter(input);
    let stripped = strip_comments_outside_fences(body);
    normalize_trailing(&stripped)
}

fn strip_frontmatter(input: &str) -> &str {
    let mut lines = input.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return input;
    };
    if first.trim_end() != "---" {
        return input;
    }
    let mut offset = first.len();
    for line in lines {
        let end = offset + line.len();
        if line.trim_end() == "---" {
            return &input[end..];
        }
        offset = end;
    }
    input
}

fn strip_comments_outside_fences(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut fence: Option<&str> = None;
    let mut in_comment = false;
    let mut offset = 0usize;

    for line in input.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();

        if in_comment {
            if let Some(pos) = line.find("-->") {
                in_comment = false;
                let resume = pos + 3;
                scan_text_segment(
                    &line[resume..],
                    line_start + resume,
                    input,
                    &mut out,
                    &mut in_comment,
                );
            }
            continue;
        }

        if let Some(marker) = fence {
            out.push_str(line);
            if line.trim_start().starts_with(marker) {
                fence = None;
            }
            continue;
        }

        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            fence = Some("```");
            out.push_str(line);
            continue;
        }
        if trimmed.starts_with("~~~") {
            fence = Some("~~~");
            out.push_str(line);
            continue;
        }

        scan_text_segment(line, line_start, input, &mut out, &mut in_comment);
    }
    out
}

fn scan_text_segment(
    segment: &str,
    segment_start: usize,
    full_input: &str,
    out: &mut String,
    in_comment: &mut bool,
) {
    let mut rest = segment;
    let mut rest_start = segment_start;
    loop {
        let Some(open) = rest.find("<!--") else {
            out.push_str(rest);
            return;
        };
        let after_open = open + 4;
        match rest[after_open..].find("-->") {
            Some(close) => {
                out.push_str(&rest[..open]);
                let resume = after_open + close + 3;
                rest_start += resume;
                rest = &rest[resume..];
            }
            None => {
                if full_input[rest_start + after_open..].contains("-->") {
                    out.push_str(&rest[..open]);
                    *in_comment = true;
                } else {
                    out.push_str(rest);
                }
                return;
            }
        }
    }
}

fn normalize_trailing(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 1);
    for line in input.split_inclusive('\n') {
        let (content, ending) = split_line_ending(line);
        out.push_str(content.trim_end_matches([' ', '\t']));
        out.push_str(ending);
    }
    while out.ends_with('\n') || out.ends_with('\r') {
        out.pop();
    }
    if out.trim().is_empty() {
        return String::new();
    }
    out.push('\n');
    out
}

fn split_line_ending(line: &str) -> (&str, &str) {
    if let Some(stripped) = line.strip_suffix("\r\n") {
        (stripped, "\r\n")
    } else if let Some(stripped) = line.strip_suffix('\n') {
        (stripped, "\n")
    } else {
        (line, "")
    }
}
