//! Amazon EventStream framing for the Bedrock ConverseStream response.
use std::io::Read;

use serde_json::Value;

use super::{invalid_stream, read_failure, Failure, MAX_EVENT_BYTES};

pub(super) fn next(reader: &mut impl Read) -> Result<Option<(String, Value)>, Failure> {
    let mut prelude = [0_u8; 12];
    if reader.read(&mut prelude[..1]).map_err(read_failure)? == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut prelude[1..]).map_err(read_failure)?;
    let total = word(&prelude[0..4]) as usize;
    let headers_len = word(&prelude[4..8]) as usize;
    if !(16..=MAX_EVENT_BYTES).contains(&total)
        || headers_len > total - 16
        || crc32fast::hash(&prelude[..8]) != word(&prelude[8..])
    {
        return Err(invalid_stream());
    }
    let mut frame = vec![0; total - 12];
    reader.read_exact(&mut frame).map_err(read_failure)?;
    let (content, checksum) = frame.split_at(frame.len() - 4);
    let mut crc = crc32fast::Hasher::new();
    crc.update(&prelude);
    crc.update(content);
    if crc.finalize() != word(checksum) {
        return Err(invalid_stream());
    }
    let (headers, payload) = content.split_at(headers_len);
    let name = event_name(headers).ok_or_else(invalid_stream)?;
    let value = if name == "bedrock.exception" {
        Value::Null
    } else {
        serde_json::from_slice(payload).map_err(|_| invalid_stream())?
    };
    Ok(Some((name.to_owned(), value)))
}

fn event_name(mut headers: &[u8]) -> Option<&str> {
    let mut event = None;
    let mut message = None;
    while !headers.is_empty() {
        let (length, rest) = headers.split_first()?;
        let (name, rest) = rest.split_at_checked(*length as usize)?;
        let (&tag, rest) = rest.split_first()?;
        let (value, rest) = match tag {
            0 | 1 => (&rest[..0], rest),
            2 => rest.split_at_checked(1)?,
            3 => rest.split_at_checked(2)?,
            4 => rest.split_at_checked(4)?,
            5 | 8 => rest.split_at_checked(8)?,
            6 | 7 => {
                let (length, rest) = rest.split_at_checked(2)?;
                let length = u16::from_be_bytes(length.try_into().ok()?) as usize;
                rest.split_at_checked(length)?
            }
            9 => rest.split_at_checked(16)?,
            _ => return None,
        };
        if tag == 7 {
            match name {
                b":event-type" => event = Some(std::str::from_utf8(value).ok()?),
                b":message-type" => message = Some(std::str::from_utf8(value).ok()?),
                _ => {}
            }
        }
        headers = rest;
    }
    match message? {
        "event" => event,
        "exception" | "error" => Some("bedrock.exception"),
        _ => None,
    }
}

fn word(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().expect("four-byte EventStream word"))
}
