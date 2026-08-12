use std::io::{Read, Write};

use anyhow::{Result, anyhow};
use bytes::Buf;

pub(crate) struct CRCWriter<W: Write> {
    hasher: crc32fast::Hasher,
    writer: W,
}

impl<W: Write> Write for CRCWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let result = self.writer.write(buf);
        if let Ok(count) = result {
            self.hasher.update(&buf[..count]);
        }
        result
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

impl<W: Write> CRCWriter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            hasher: crc32fast::Hasher::new(),
            writer,
        }
    }

    pub fn finalize(mut self) -> Result<[u8; 4]> {
        let crc = self.hasher.finalize().to_be_bytes();
        self.writer.write_all(crc.as_slice())?;
        Ok(crc)
    }
}

pub(crate) struct CRCReader<R: Read> {
    hasher: crc32fast::Hasher,
    reader: R,
}

impl<R: Read> CRCReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            hasher: crc32fast::Hasher::new(),
            reader,
        }
    }

    pub fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        let result = self.reader.read_exact(buf);
        if result.is_ok() {
            self.hasher.update(buf);
        }
        result
    }

    pub fn read_u16(&mut self) -> std::io::Result<u16> {
        let mut result = [0u8; 2];
        self.read_exact(result.as_mut_slice())?;
        Ok(result.as_slice().get_u16())
    }

    pub fn check(mut self) -> Result<()> {
        let mut expected_crc = [0u8; 4];
        self.reader.read_exact(expected_crc.as_mut_slice())?;

        let expected_crc = expected_crc.as_slice().get_u32();
        let crc = self.hasher.finalize();
        if crc == expected_crc {
            Ok(())
        } else {
            Err(error(expected_crc, crc))
        }
    }
}

pub fn check_crc(buf: &[u8]) -> Result<&[u8]> {
    let crc_offset = buf.len() - size_of::<u32>();
    let crc = crc32fast::hash(&buf[..crc_offset]);
    let expected_crc = (&buf[crc_offset..]).get_u32();
    if expected_crc == crc {
        Ok(&buf[..crc_offset])
    } else {
        Err(error(expected_crc, crc))
    }
}

fn error(expected: u32, got: u32) -> anyhow::Error {
    anyhow!("CRC mismatch: expected {}, got {}", expected, got)
}
