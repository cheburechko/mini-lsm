use anyhow::{Result, anyhow};
use bytes::Buf;

pub fn check_crc(buf: &[u8]) -> Result<&[u8]> {
    let crc_offset = buf.len() - size_of::<u32>();
    let crc = crc32fast::hash(&buf[..crc_offset]);
    let expected_crc = (&buf[crc_offset..]).get_u32();
    if expected_crc == crc {
        Ok(&buf[..crc_offset])
    } else {
        Err(anyhow!(
            "CRC mismatch: expected {}, got {}",
            expected_crc,
            crc
        ))
    }
}
