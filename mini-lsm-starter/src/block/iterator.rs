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

use std::sync::Arc;

use crate::{
    block::U16_SIZE,
    key::{KeySlice, KeyVec},
};
use bytes::Buf;

use super::Block;

/// Iterates on a block.
pub struct BlockIterator {
    /// The internal `Block`, wrapped by an `Arc`
    block: Arc<Block>,
    /// The current key, empty represents the iterator is invalid
    key: KeyVec,
    /// the current value range in the block.data, corresponds to the current key
    value_range: (usize, usize),
    /// Current index of the key-value pair, should be in range of [0, num_of_elements)
    idx: usize,
    /// The first key in the block
    first_key: KeyVec,
}

fn read_value<'slice>(buf: &mut &'slice [u8]) -> &'slice [u8] {
    let len = buf.get_u16() as usize;
    let result = &buf[..len];
    buf.advance(len);
    result
}

fn get_entry(block: &Block, idx: usize) -> (KeySlice<'_>, (usize, usize)) {
    let offset = block.offsets.get(idx);
    if let Some(offset) = offset {
        let offset = *offset as usize;
        let mut entry_buf = &block.data[offset..];
        let key = read_value(&mut entry_buf);
        let value = read_value(&mut entry_buf);
        let value_start = offset + key.len() + U16_SIZE * 2;
        (
            KeySlice::from_slice(key),
            (value_start, value_start + value.len()),
        )
    } else {
        (KeySlice::from_slice(&[]), (0, 0))
    }
}

impl BlockIterator {
    fn new(block: Arc<Block>) -> Self {
        let first_key = KeyVec::from_vec(read_value(&mut block.data.as_slice()).to_vec());
        Self {
            block,
            key: KeyVec::new(),
            value_range: (0, 0),
            idx: 0,
            first_key,
        }
    }

    /// Creates a block iterator and seek to the first entry.
    pub fn create_and_seek_to_first(block: Arc<Block>) -> Self {
        let mut block = Self::new(block);
        block.seek_to_first();
        block
    }

    /// Creates a block iterator and seek to the first key that >= `key`.
    pub fn create_and_seek_to_key(block: Arc<Block>, key: KeySlice) -> Self {
        let mut block = Self::new(block);
        block.seek_to_key(key);
        block
    }

    /// Returns the key of the current entry.
    pub fn key(&self) -> KeySlice<'_> {
        KeySlice::from_slice(self.key.raw_ref())
    }

    /// Returns the value of the current entry.
    pub fn value(&self) -> &[u8] {
        &self.block.data[self.value_range.0..self.value_range.1]
    }

    /// Returns true if the iterator is valid.
    /// Note: You may want to make use of `key`
    pub fn is_valid(&self) -> bool {
        self.idx < self.len()
    }

    /// Seeks to the first key in the block.
    pub fn seek_to_first(&mut self) {
        self.seek(0);
    }

    /// Move to the next key in the block.
    pub fn next(&mut self) {
        if self.is_valid() {
            self.seek(self.idx + 1);
        }
    }

    /// Seek to the first key that >= `key`.
    /// Note: You should assume the key-value pairs in the block are sorted when being added by
    /// callers.
    pub fn seek_to_key(&mut self, key: KeySlice) {
        let idx = self.block.offsets.partition_point(|offset| {
            let mut buf = &self.block.data[(*offset as usize)..];
            read_value(&mut buf) < key.raw_ref()
        });
        self.seek(idx);
    }

    fn seek(&mut self, idx: usize) {
        self.idx = idx;

        let (key, value_range) = get_entry(&self.block, idx);

        self.key.set_from_slice(key);

        self.value_range = value_range;
    }

    fn len(&self) -> usize {
        self.block.offsets.len()
    }
}
