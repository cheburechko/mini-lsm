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

use std::sync::Arc;

use anyhow::Result;

use super::StorageIterator;
use crate::{
    key::KeySlice,
    table::{SsTable, SsTableIterator},
};

/// Concat multiple iterators ordered in key order and their key ranges do not overlap. We do not want to create the
/// iterators when initializing this iterator to reduce the overhead of seeking.
pub struct SstConcatIterator {
    current: Option<SsTableIterator>,
    next_sst_idx: usize,
    sstables: Vec<Arc<SsTable>>,
}

impl SstConcatIterator {
    pub fn create_and_seek_to_first(sstables: Vec<Arc<SsTable>>) -> Result<Self> {
        let current = if let Some(table) = sstables.first() {
            Some(SsTableIterator::create_and_seek_to_first(Arc::clone(
                table,
            ))?)
        } else {
            None
        };
        Ok(SstConcatIterator {
            current,
            next_sst_idx: 1,
            sstables,
        })
    }

    pub fn create_and_seek_to_key(sstables: Vec<Arc<SsTable>>, key: KeySlice) -> Result<Self> {
        let pos = sstables
            .partition_point(|table| table.first_key().as_key_slice() <= key)
            .saturating_sub(1);
        let current = if let Some(table) = sstables.get(pos) {
            Some(SsTableIterator::create_and_seek_to_key(
                Arc::clone(table),
                key,
            )?)
        } else {
            None
        };
        Ok(SstConcatIterator {
            current,
            next_sst_idx: pos + 1,
            sstables,
        })
    }
}

impl StorageIterator for SstConcatIterator {
    type KeyType<'a> = KeySlice<'a>;

    fn key(&self) -> KeySlice<'_> {
        if let Some(ref iter) = self.current {
            iter.key()
        } else {
            KeySlice::from_slice(b"")
        }
    }

    fn value(&self) -> &[u8] {
        if let Some(ref iter) = self.current {
            iter.value()
        } else {
            b""
        }
    }

    fn is_valid(&self) -> bool {
        self.current.as_ref().is_some_and(|iter| iter.is_valid())
    }

    fn next(&mut self) -> Result<()> {
        self.current = if let Some(mut iter) = self.current.take() {
            iter.next()?;
            if iter.is_valid() {
                Some(iter)
            } else {
                if let Some(table) = self.sstables.get(self.next_sst_idx) {
                    self.next_sst_idx += 1;
                    Some(SsTableIterator::create_and_seek_to_first(Arc::clone(
                        table,
                    ))?)
                } else {
                    None
                }
            }
        } else {
            None
        };
        Ok(())
    }

    fn num_active_iterators(&self) -> usize {
        1
    }
}
