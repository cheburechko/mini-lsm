// REMOVE THIS LINE after fully implementing this functionality
// Copyright (c) 2022-2025 Alex Chi Z
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
#![allow(unused_variables)] // TODO(you): remove this lint after implementing this mod
#![allow(dead_code)] // TODO(you): remove this lint after implementing this mod

use anyhow::Result;
use bytes::Bytes;
use crossbeam_skiplist::SkipMap;
use parking_lot::Mutex;
use std::fs::{File, OpenOptions};
use std::io::ErrorKind::UnexpectedEof;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::Arc;

use crate::crc::{CRCReader, CRCWriter};
use crate::key::KeySlice;

pub struct Wal {
    file: Arc<Mutex<BufWriter<File>>>,
}

impl Wal {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            file: Arc::new(Mutex::new(BufWriter::new(
                OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .create(true)
                    .open(path)?,
            ))),
        })
    }

    pub fn recover(path: impl AsRef<Path>, skiplist: &SkipMap<Bytes, Bytes>) -> Result<Self> {
        let mut reader = BufReader::new(File::open(&path)?);

        loop {
            let mut crc_reader = CRCReader::new(&mut reader);
            let key = Self::read(&mut crc_reader);
            if let Err(ref e) = key
                && e.kind() == UnexpectedEof
            {
                break;
            }
            let key = key?;
            let value = Self::read(&mut crc_reader)?;
            crc_reader.check()?;
            skiplist.insert(key, value);
        }

        Ok(Self {
            file: Arc::new(Mutex::new(BufWriter::new(
                OpenOptions::new().append(true).open(path)?,
            ))),
        })
    }

    fn read<R: Read>(reader: &mut CRCReader<R>) -> Result<Bytes, std::io::Error> {
        let mut buf = Vec::new();

        let len = reader.read_u16()?;
        buf.resize(len as usize, 0);
        reader.read_exact(buf.as_mut_slice())?;
        Ok(Bytes::from(buf))
    }

    fn write<W: Write>(writer: &mut W, value: &[u8]) -> Result<()> {
        writer.write_all((value.len() as u16).to_be_bytes().as_slice())?;
        writer.write_all(value)?;
        Ok(())
    }

    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut lock = self.file.lock();
        let mut writer = CRCWriter::new(lock.by_ref());
        Self::write(&mut writer, key)?;
        Self::write(&mut writer, value)?;

        writer.finalize()?;

        Ok(())
    }

    /// Implement this in week 3, day 5; if you want to implement this earlier, use `&[u8]` as the key type.
    pub fn put_batch(&self, _data: &[(KeySlice, &[u8])]) -> Result<()> {
        unimplemented!()
    }

    pub fn sync(&self) -> Result<()> {
        let mut lock = self.file.lock();
        lock.flush()?;
        lock.get_mut().sync_all()?;
        Ok(())
    }
}
