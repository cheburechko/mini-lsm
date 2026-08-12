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

use std::fs::OpenOptions;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use std::{fs::File, io::Write};

use anyhow::Result;
use parking_lot::{Mutex, MutexGuard};
use serde::{Deserialize, Serialize};

use crate::compact::CompactionTask;
use crate::crc::{CRCReader, CRCWriter};

pub struct Manifest {
    file: Arc<Mutex<File>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ManifestRecord {
    Flush(usize),
    NewMemtable(usize),
    Compaction(CompactionTask, Vec<usize>),
}

impl Manifest {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            file: Arc::new(Mutex::new(File::create(path)?)),
        })
    }

    pub fn recover(path: impl AsRef<Path>) -> Result<(Self, Vec<ManifestRecord>)> {
        let mut manifest = Vec::new();
        {
            let mut reader = BufReader::new(File::open(&path)?);
            let mut buf = Vec::new();
            loop {
                let mut crc_reader = CRCReader::new(&mut reader);
                let len = crc_reader.read_u16();
                if len.is_err() {
                    break;
                }
                buf.resize(len? as usize, 0);
                crc_reader.read_exact(buf.as_mut_slice())?;
                crc_reader.check()?;
                manifest.push(serde_json::from_slice(&buf)?);
            }
        }
        Ok((
            Self {
                file: Arc::new(Mutex::new(OpenOptions::new().append(true).open(path)?)),
            },
            manifest,
        ))
    }

    pub fn add_record(
        &self,
        _state_lock_observer: &MutexGuard<()>,
        record: ManifestRecord,
    ) -> Result<()> {
        self.add_record_when_init(record)
    }

    pub fn add_record_when_init(&self, record: ManifestRecord) -> Result<()> {
        let mut file = self.file.lock();
        let mut writer = CRCWriter::new(file.by_ref());
        let value = serde_json::to_vec(&record)?;
        writer.write_all((value.len() as u16).to_be_bytes().as_slice())?;
        writer.write_all(value.as_slice())?;
        writer.finalize()?;
        file.sync_all()?;
        Ok(())
    }
}
