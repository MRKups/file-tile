//! Bound RIFF ranges and image dimensions before WebP decoder/metadata allocation.
use super::preview::dimensions;

pub(super) fn validate(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 20 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err("Cannot thumbnail WebP: invalid RIFF header. Use Open.".into());
    }
    let total = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as u64 + 8;
    if total != bytes.len() as u64 {
        return Err("Cannot thumbnail WebP: truncated or inconsistent RIFF size. Use Open.".into());
    }
    chunks(&bytes[12..], true)
}

fn three(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) | u32::from(bytes[1]) << 8 | u32::from(bytes[2]) << 16
}

fn chunks(mut bytes: &[u8], frames: bool) -> Result<(), String> {
    while !bytes.is_empty() {
        if bytes.len() < 8 {
            return Err("Cannot thumbnail WebP: truncated chunk header. Use Open.".into());
        }
        let tag = &bytes[..4];
        let length = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as u64;
        let end = 8 + length;
        let padded = end + (length & 1);
        if padded > bytes.len() as u64 {
            return Err("Cannot thumbnail WebP: chunk exceeds the input file. Use Open.".into());
        }
        let data = &bytes[8..end as usize];
        match tag {
            b"VP8X" if data.len() >= 10 => {
                dimensions(three(&data[4..7]) + 1, three(&data[7..10]) + 1)?
            }
            b"VP8L" if data.len() >= 5 => {
                let bits = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);
                dimensions((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1)?;
            }
            b"VP8 " if data.len() >= 10 => dimensions(
                u32::from(u16::from_le_bytes([data[6], data[7]]) & 0x3fff),
                u32::from(u16::from_le_bytes([data[8], data[9]]) & 0x3fff),
            )?,
            b"ANMF" if frames && data.len() >= 16 => {
                dimensions(three(&data[6..9]) + 1, three(&data[9..12]) + 1)?;
                chunks(&data[16..], false)?;
            }
            b"VP8X" | b"VP8L" | b"VP8 " | b"ANMF" => {
                return Err("Cannot thumbnail WebP: invalid image/frame header. Use Open.".into());
            }
            _ => {}
        }
        bytes = &bytes[padded as usize..];
    }
    Ok(())
}
