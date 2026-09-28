//! Bounded, agent-safe excerpts of tool output.
//!
//! Tool diagnostics can be arbitrarily large and full of terminal escape
//! sequences and absolute paths. Agent-facing messages should instead carry a
//! short, plain-text, workspace-relative excerpt plus a pointer to the full
//! log. The helpers here are pure so both the deferred and the immediate
//! runner can share them.

use std::path::Path;

/// A bounded excerpt and whether anything was cut to fit its limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Excerpt {
    /// Plain-text excerpt without a trailing newline.
    pub text: String,
    /// Whether lines or characters were omitted.
    pub truncated: bool,
}

/// Remove ANSI/VT escape sequences and normalize carriage returns.
pub(crate) fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\u{1b}' => match chars.next() {
                // CSI: parameters and intermediates end at a final byte in
                // the range `@`..=`~`.
                Some('[') => {
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                // OSC: terminated by BEL or ESC `\`.
                Some(']') => {
                    while let Some(next) = chars.next() {
                        if next == '\u{7}' {
                            break;
                        }
                        if next == '\u{1b}' {
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                // Other two-character escapes (charset selection, etc.).
                Some(_) | None => {}
            },
            '\r' => {
                if chars.peek() != Some(&'\n') {
                    out.push('\n');
                }
            }
            _ => out.push(character),
        }
    }
    out
}

/// Rewrite absolute paths under any of `roots` to root-relative paths.
///
/// Longer roots are applied first so nested workspaces win over their parents.
pub(crate) fn relativize(input: &str, roots: &[&Path]) -> String {
    let mut prefixes = roots
        .iter()
        .map(|root| {
            let mut prefix = root.to_string_lossy().replace('\\', "/");
            if !prefix.ends_with('/') {
                prefix.push('/');
            }
            prefix
        })
        .filter(|prefix| prefix.len() > 1)
        .collect::<Vec<_>>();
    prefixes.sort_by_key(|prefix| std::cmp::Reverse(prefix.len()));
    prefixes.dedup();
    let mut out = input.to_owned();
    for prefix in prefixes {
        out = out.replace(&prefix, "");
    }
    out
}

/// Plain text used for display and fingerprinting: escape sequences removed,
/// paths relativized, trailing whitespace trimmed per line, and surrounding
/// blank lines removed.
pub(crate) fn normalize(input: &str, roots: &[&Path]) -> String {
    let text = relativize(&strip_ansi(input), roots);
    let lines = text.lines().map(str::trim_end).collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| !line.is_empty())
        .unwrap_or(lines.len());
    let end = lines
        .iter()
        .rposition(|line| !line.is_empty())
        .map_or(start, |index| index + 1);
    lines[start..end].join("\n")
}

/// Longest single line kept in an excerpt; longer lines end in `…`.
const MAX_LINE_CHARS: usize = 400;

/// Keep at most `max_lines` lines and `max_chars` characters of `text`,
/// shortening any single line longer than [`MAX_LINE_CHARS`].
pub(crate) fn clip(text: &str, max_lines: usize, max_chars: usize) -> Excerpt {
    let mut out = String::new();
    let mut chars = 0usize;
    let mut truncated = false;
    for (index, line) in text.lines().enumerate() {
        if index >= max_lines {
            truncated = true;
            break;
        }
        let shortened;
        let line = if line.chars().count() > MAX_LINE_CHARS {
            truncated = true;
            shortened = line
                .chars()
                .take(MAX_LINE_CHARS)
                .chain(['…'])
                .collect::<String>();
            shortened.as_str()
        } else {
            line
        };
        let separator = usize::from(index > 0);
        let line_chars = line.chars().count();
        if chars + separator + line_chars > max_chars {
            let room = max_chars.saturating_sub(chars + separator);
            if room > 0 {
                if separator == 1 {
                    out.push('\n');
                }
                out.extend(line.chars().take(room));
            }
            truncated = true;
            break;
        }
        if separator == 1 {
            out.push('\n');
        }
        out.push_str(line);
        chars += separator + line_chars;
    }
    Excerpt {
        text: out,
        truncated,
    }
}

/// Fewest lines one excerpt gets from a shared budget when several share it.
pub(crate) const MIN_SHARE_LINES: usize = 5;
/// Fewest characters one excerpt gets from a shared budget.
pub(crate) const MIN_SHARE_CHARS: usize = 400;

/// Clip each text to a fair share of one `max_lines`/`max_chars` budget:
/// every text gets an equal share (at least [`MIN_SHARE_LINES`] and
/// [`MIN_SHARE_CHARS`]), never more than what earlier texts left. Stop and
/// immediate mode divide their agent excerpt budget this way.
pub(crate) fn clip_shared(texts: &[String], max_lines: usize, max_chars: usize) -> Vec<Excerpt> {
    let count = texts.len().max(1);
    let share_lines = (max_lines / count).max(MIN_SHARE_LINES);
    let share_chars = (max_chars / count).max(MIN_SHARE_CHARS);
    let (mut used_lines, mut used_chars) = (0usize, 0usize);
    texts
        .iter()
        .map(|text| {
            let clipped = clip(
                text,
                share_lines.min(max_lines.saturating_sub(used_lines)),
                share_chars.min(max_chars.saturating_sub(used_chars)),
            );
            used_lines += clipped.text.lines().count();
            used_chars += clipped.text.chars().count();
            clipped
        })
        .collect()
}

/// Render an excerpt followed by a pointer to the full log when needed.
pub(crate) fn with_log_note(excerpt: &Excerpt, log_path: Option<&str>) -> String {
    let log = log_path.map_or_else(String::new, |path| format!("; full log: {path}"));
    match (excerpt.text.is_empty(), excerpt.truncated) {
        (true, false) => format!("(no diagnostic output{log})"),
        (true, true) => format!("(output omitted{log})"),
        (false, true) => format!("{}\n…truncated{log}", excerpt.text),
        (false, false) => excerpt.text.clone(),
    }
}

/// Stable 64-bit FNV-1a digest, rendered as hex, for persisted fingerprints.
pub(crate) fn fingerprint(parts: impl IntoIterator<Item = impl AsRef<[u8]>>) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for part in parts {
        for byte in part.as_ref().iter().copied().chain([0xff]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_csi_osc_and_carriage_returns() {
        let raw = "\u{1b}[1;31merror\u{1b}[0m: \u{1b}]8;;file:///x\u{7}link\u{1b}]8;;\u{1b}\\ done\r\nnext\rover";
        assert_eq!(strip_ansi(raw), "error: link done\nnext\nover");
    }

    #[test]
    fn relativizes_nested_roots_first() {
        let text = "/repo/sub/a.py:1 and /repo/b.py:2 and /repository/c.py";
        let roots = [Path::new("/repo"), Path::new("/repo/sub")];
        assert_eq!(
            relativize(text, &roots),
            "a.py:1 and b.py:2 and /repository/c.py"
        );
    }

    #[test]
    fn normalize_trims_blank_edges_and_trailing_spaces() {
        let raw = "\n\n  first  \nsecond\t\n\n";
        assert_eq!(normalize(raw, &[]), "  first\nsecond");
    }

    #[test]
    fn clip_bounds_lines_and_characters() {
        let text = "one\ntwo\nthree\nfour";
        assert_eq!(
            clip(text, 2, 100),
            Excerpt {
                text: "one\ntwo".into(),
                truncated: true
            }
        );
        assert_eq!(
            clip(text, 10, 9),
            Excerpt {
                text: "one\ntwo\nt".into(),
                truncated: true
            }
        );
        assert_eq!(
            clip(text, 10, 100),
            Excerpt {
                text: text.into(),
                truncated: false
            }
        );
        let wide = "é".repeat(10);
        assert_eq!(clip(&wide, 1, 3).text, "ééé");
        let long_line = format!("{}\nnext", "x".repeat(10_000));
        let clipped = clip(&long_line, 10, 100_000);
        assert!(clipped.truncated);
        assert_eq!(
            clipped.text,
            format!("{}…\nnext", "x".repeat(MAX_LINE_CHARS))
        );
    }

    #[test]
    fn shared_budget_gives_every_text_a_fair_share() {
        let long = (0..40)
            .map(|n| format!("a{n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let clipped = clip_shared(&[long.clone(), "b1\nb2".into()], 20, 10_000);
        assert_eq!(clipped[0].text.lines().count(), 10);
        assert!(clipped[0].truncated);
        assert_eq!(clipped[1].text, "b1\nb2");
        // A later text is not starved by an earlier long one.
        let clipped = clip_shared(&[long.clone(), long, "c1".into()], 30, 10_000);
        assert_eq!(clipped[2].text, "c1");
    }

    #[test]
    fn log_note_marks_truncation_and_empty_output() {
        let cut = Excerpt {
            text: "a".into(),
            truncated: true,
        };
        assert_eq!(
            with_log_note(&cut, Some("/logs/x.log")),
            "a\n…truncated; full log: /logs/x.log"
        );
        let empty = Excerpt {
            text: String::new(),
            truncated: false,
        };
        assert_eq!(
            with_log_note(&empty, Some("/logs/x.log")),
            "(no diagnostic output; full log: /logs/x.log)"
        );
        let omitted = Excerpt {
            text: String::new(),
            truncated: true,
        };
        assert_eq!(with_log_note(&omitted, None), "(output omitted)");
    }

    #[test]
    fn fingerprint_is_stable_and_part_sensitive() {
        assert_eq!(fingerprint(["ab", "c"]), fingerprint(["ab", "c"]));
        assert_ne!(fingerprint(["ab", "c"]), fingerprint(["a", "bc"]));
        assert_eq!(fingerprint(Vec::<&str>::new()).len(), 16);
    }
}
