//! Copyable CLI launch commands, grouped for reading in an interactive terminal.
use std::{collections::BTreeMap, path::Path};

const LINE_WIDTH: usize = 96;

fn quoted_word(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn assignment_value(value: &str) -> String {
    // Deliberately exclude expansion, glob, comment and command syntax. Empty
    // arguments must remain visible; quoted values retain their exact bytes.
    if !value.is_empty() && value.bytes().all(|byte|
        byte.is_ascii_alphanumeric() || b"_./:@%+,-".contains(&byte)) {
        value.to_owned()
    } else {
        quoted_word(value)
    }
}

/// Splits a command line or composite argument into individual words, respecting single and double quotes
/// as well as backslash escapes. This allows flags and their arguments (e.g. `--system-prompt '/path/to/file'`)
/// or whole command-line fragments to be tokenized properly instead of passed as a single quoted option name.
pub(crate) fn split_argument(arg: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = arg.chars().peekable();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut has_token = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
                has_token = true;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
                has_token = true;
            }
            '\\' if in_double_quote => {
                if let Some(&next) = chars.peek() {
                    if next == '"' || next == '\\' || next == '$' || next == '`' {
                        chars.next();
                        current.push(next);
                        has_token = true;
                        continue;
                    }
                }
                current.push('\\');
                has_token = true;
            }
            '\\' if !in_single_quote => {
                if let Some(next) = chars.next() {
                    current.push(next);
                    has_token = true;
                }
            }
            c if (c == ' ' || c == '\t' || c == '\r' || c == '\n') && !in_single_quote && !in_double_quote => {
                if has_token || !current.is_empty() {
                    words.push(current);
                    current = String::new();
                    has_token = false;
                }
            }
            c => {
                current.push(c);
                has_token = true;
            }
        }
    }
    if has_token || !current.is_empty() {
        words.push(current);
    }
    words
}

pub(crate) fn normalize_args(args: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for arg in args {
        let trimmed = arg.trim();
        if trimmed.is_empty() {
            continue;
        }
        let split = split_argument(trimmed);
        if split.is_empty() {
            normalized.push(trimmed.to_string());
        } else {
            normalized.extend(split);
        }
    }
    normalized
}

pub(super) fn cli_command(
    program: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    directory: &Path,
) -> Result<String, String> {
    let bin = Path::new(program).parent().ok_or("无效的可执行程序路径")?;
    let mut command = format!("cd -- {} &&\n", quoted_word(&directory.to_string_lossy()));
    command.push_str(&format!("  PATH={}:\"$PATH\" \\\n", assignment_value(&bin.to_string_lossy())));
    for (name, value) in env {
        command.push_str(&format!("  {name}={} \\\n", assignment_value(value)));
    }

    // Preserve the resolved absolute executable: a prettier basename could
    // select an alias, function or a different executable in the user's shell.
    // Quote even plain arguments to suppress zsh global aliases (e.g. G).
    let program = quoted_word(program);
    command.push_str("  ");
    command.push_str(&program);
    let mut width = 2 + program.chars().count();
    let normalized = normalize_args(args);
    for argument in &normalized {
        let word = quoted_word(argument);
        let length = word.chars().count();
        if width + 1 + length > LINE_WIDTH {
            command.push_str(" \\\n    ");
            width = 4;
        } else {
            command.push(' ');
            width += 1;
        }
        command.push_str(&word);
        width += length;
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_argument_handles_quotes_and_composite_options() {
        assert_eq!(
            split_argument("--system-prompt '/Users/scoheart/Code/english/ENGLISH.md'"),
            vec!["--system-prompt", "/Users/scoheart/Code/english/ENGLISH.md"]
        );
        assert_eq!(
            split_argument("--system-prompt \"/Users/scoheart/Code/english/ENGLISH.md\""),
            vec!["--system-prompt", "/Users/scoheart/Code/english/ENGLISH.md"]
        );
        assert_eq!(
            split_argument("--system-prompt /Users/scoheart/Code/english/ENGLISH.md"),
            vec!["--system-prompt", "/Users/scoheart/Code/english/ENGLISH.md"]
        );
        assert_eq!(
            split_argument("-m \"hello world\" --flag"),
            vec!["-m", "hello world", "--flag"]
        );
        assert_eq!(
            split_argument("web"),
            vec!["web"]
        );
        assert_eq!(
            split_argument("'/Users/scoheart/Code/english/ENGLISH.md'"),
            vec!["/Users/scoheart/Code/english/ENGLISH.md"]
        );
        assert_eq!(
            split_argument("\"path with spaces/file\""),
            vec!["path with spaces/file"]
        );
        assert_eq!(
            split_argument("path\\ with\\ spaces/file"),
            vec!["path with spaces/file"]
        );
    }

    #[test]
    fn cli_command_normalizes_composite_arguments_into_separate_shell_words() {
        let mut env = BTreeMap::new();
        env.insert("ALL_PROXY".to_string(), "http://127.0.0.1:8080".to_string());
        let line = cli_command(
            "/Users/scoheart/.local/bin/pi",
            &["--system-prompt '/Users/scoheart/Code/english/ENGLISH.md'".to_string()],
            &env,
            Path::new("/Users/scoheart/Code/test"),
        ).unwrap();

        assert!(line.contains("'/Users/scoheart/.local/bin/pi'"));
        assert!(line.contains("'--system-prompt'"));
        assert!(line.contains("'/Users/scoheart/Code/english/ENGLISH.md'"));
        assert!(!line.contains("'--system-prompt \\''/Users/scoheart/Code/english/ENGLISH.md\\'"));
    }
}
