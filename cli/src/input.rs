//! Bounded local input for typed lifecycle commands. Secret values never enter argv.

use std::fs::File;
use std::io::{self, IsTerminal, Read};
use std::path::Path;

use serde_json::Value;

use crate::output::CliError;

fn read_bounded(reader: impl Read, limit: u64) -> Result<String, CliError> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)
        .map_err(|error| CliError::private("input_read_failed", error.to_string()))?;
    if bytes.len() as u64 > limit {
        return Err(CliError::new("input_too_large", "input exceeds the supported size limit"));
    }
    String::from_utf8(bytes)
        .map_err(|_| CliError::new("invalid_input", "input must be UTF-8"))
}

pub fn json_file(path: &Path) -> Result<Value, CliError> {
    let contents = if path == Path::new("-") {
        read_bounded(io::stdin().lock(), 1024 * 1024)?
    } else {
        let file = File::open(path)
            .map_err(|error| CliError::private("input_read_failed", error.to_string()))?;
        read_bounded(file, 1024 * 1024)?
    };
    serde_json::from_str(&contents)
        .map_err(|_| CliError::new("invalid_json", "input must be a valid JSON document"))
}

pub fn provider_credential(
    path: &Path,
    from_stdin: bool,
    clear: bool,
) -> Result<Option<String>, CliError> {
    if from_stdin && clear {
        return Err(CliError::new("option_conflict", "choose either --credential-stdin or --clear-credential"));
    }
    if from_stdin {
        if path == Path::new("-") {
            return Err(CliError::new("option_conflict", "--file - and --credential-stdin cannot share stdin"));
        }
        if io::stdin().is_terminal() {
            return Err(CliError::new("credential_input_required", "pipe the credential to stdin; terminal echo is not supported"));
        }
        let mut credential = read_bounded(io::stdin().lock(), 64 * 1024)?;
        let length = credential.trim_end_matches(['\r', '\n']).len();
        if length == 0 {
            return Err(CliError::new("empty_credential", "empty stdin does not clear a credential; use --clear-credential"));
        }
        credential.truncate(length);
        return Ok(Some(credential));
    }
    Ok(clear.then(String::new))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_input_and_invalid_utf8_fail_closed() {
        assert_eq!(read_bounded(&b"12345"[..], 4).unwrap_err().code, "input_too_large");
        assert_eq!(read_bounded(&[255][..], 4).unwrap_err().code, "invalid_input");
        assert_eq!(read_bounded(&b"1234"[..], 4).unwrap(), "1234");
    }

    #[test]
    fn competing_stdin_consumers_are_rejected_before_reading() {
        assert_eq!(provider_credential(Path::new("-"), true, false).unwrap_err().code, "option_conflict");
        assert_eq!(provider_credential(Path::new("provider.json"), true, true).unwrap_err().code, "option_conflict");
    }
}
