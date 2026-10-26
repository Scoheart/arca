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
    for argument in args {
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
