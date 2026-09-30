//! Error constructors and one-line error summaries shared by both runners.

use crate::excerpt;
use hookkit_core::HookkitError;

pub(crate) fn state_error(error: hookkit_session_state::StateError) -> HookkitError {
    std::io::Error::other(error).into()
}

pub(crate) fn activity_error(error: hookkit_file_activity::FileActivityError) -> HookkitError {
    std::io::Error::other(error).into()
}

/// Longest configuration or tool error summary echoed in a user notice.
const ERROR_SUMMARY_CHARS: usize = 300;

/// One-line summary of an error: its first line, plus the first informative
/// line after it when the first only introduces the detail (`pkl eval failed
/// for <file>:` followed by Pkl's `–– Pkl Error ––` banner and message).
pub(crate) fn error_summary(detail: &str) -> String {
    let mut lines = detail
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let first = lines.next().unwrap_or_default();
    let summary = match first.strip_suffix(':') {
        Some(head) => match lines.find(|line| !line.starts_with("––") && !line.starts_with("--"))
        {
            Some(next) => format!("{head}: {}", next.trim_start_matches("- ")),
            None => head.to_owned(),
        },
        None => first.to_owned(),
    };
    excerpt::clip(&summary, 1, ERROR_SUMMARY_CHARS).text
}

pub(crate) fn invalid_data(message: String) -> HookkitError {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_summaries_keep_the_informative_line() {
        assert_eq!(
            error_summary(
                "pkl eval failed for /p/.velvet-glove/post-tool-use.pkl:\n–– Pkl Error ––\nExpected value of type `Int`, but got type `String`.\n\n2 | jobs = \"x\"\n"
            ),
            "pkl eval failed for /p/.velvet-glove/post-tool-use.pkl: Expected value of type `Int`, but got type `String`."
        );
        assert_eq!(
            error_summary(
                "invalid Velvet Glove configuration:\n- ruff (ruff): invalid file glob `src/{a`"
            ),
            "invalid Velvet Glove configuration: ruff (ruff): invalid file glob `src/{a`"
        );
        assert_eq!(error_summary("plain failure"), "plain failure");
    }
}
