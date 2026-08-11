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
use bytes::{Buf, BufMut, Bytes};
use crossbeam_skiplist::SkipMap;
use parking_lot::Mutex;
use std::fs::{File, OpenOptions};
use std::io::ErrorKind::UnexpectedEof;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::Arc;

use crate::key::KeySlice;

pub struct Wal {
    file: Arc<Mutex<BufWriter<File>>>,
}

impl Wal {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            file: Arc::new(Mutex::new(BufWriter::new(File::create_new(path)?))),
        })
    }

    pub fn recover(path: impl AsRef<Path>, skiplist: &SkipMap<Bytes, Bytes>) -> Result<Self> {
        let mut reader = BufReader::new(File::open(&path)?);

        loop {
            let key = Self::read(&mut reader);
            if key.is_err() {
                if key.as_ref().unwrap_err().kind() == UnexpectedEof {
                    break;
                }
            }
            let value = Self::read(&mut reader)?;
            skiplist.insert(key?, value);
        }

        Ok(Self {
            file: Arc::new(Mutex::new(BufWriter::new(
                OpenOptions::new().append(true).open(path)?,
            ))),
        })
    }

    fn read(reader: &mut BufReader<File>) -> Result<Bytes, std::io::Error> {
        let mut len = [0u8; 2];
        let mut buf = Vec::new();

        reader.read_exact(len.as_mut_slice())?;
        buf.resize(len.as_slice().get_u16() as usize, 0);
        reader.read_exact(buf.as_mut_slice())?;
        Ok(Bytes::from(buf))
    }

    fn write(writer: &mut BufWriter<File>, value: &[u8]) -> Result<()> {
        let mut len = [0u8; 2];

        len.as_mut_slice().put_u16(value.len() as u16);
        writer.write_all(len.as_slice())?;
        writer.write_all(value)?;
        Ok(())
    }

    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut lock = self.file.lock();
        Self::write(lock.by_ref(), key)?;
        Self::write(lock.by_ref(), value)?;
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
