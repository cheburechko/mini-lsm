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

use crate::{
    block::U16_SIZE,
    key::{KeySlice, KeyVec},
};
use bytes::BufMut;

use super::Block;

/// Builds a block.
pub struct BlockBuilder {
    /// Offsets of each key-value entries.
    offsets: Vec<u16>,
    /// All serialized key-value pairs in the block.
    data: Vec<u8>,
    /// The expected block size.
    block_size: usize,
    /// The first key in the block
    first_key: KeyVec,
}

fn get_overlap(a: &[u8], b: &[u8]) -> usize {
    let max_len = a.len().min(b.len());
    for i in 0..max_len {
        if a[i] != b[i] {
            return i;
        }
    }
    max_len
}

impl BlockBuilder {
    /// Creates a new block builder.
    pub fn new(block_size: usize) -> Self {
        BlockBuilder {
            offsets: Vec::new(),
            data: Vec::with_capacity(block_size),
            block_size,
            first_key: KeyVec::new(),
        }
    }

    /// Adds a key-value pair to the block. Returns false when the block is full.
    /// You may find the `bytes::BufMut` trait useful for manipulating binary data.
    #[must_use]
    pub fn add(&mut self, key: KeySlice, value: &[u8]) -> bool {
        let is_first_key = self.first_key.is_empty();
        if is_first_key {
            self.first_key.set_from_slice(key);
        } else {
            let to_add = 6 + key.key_len() + value.len();
            if to_add + self.get_estimated_size() > self.block_size {
                return false;
            }
        }

        self.offsets.push(self.data.len() as u16);
        if is_first_key {
            self.data.put_u16(key.key_len() as u16);
            self.data.put(key.key_ref());
        } else {
            let overlap = get_overlap(self.first_key.key_ref(), key.key_ref());
            self.data.put_u16(overlap as u16);
            self.data.put_u16((key.key_len() - overlap) as u16);
            self.data.put(&key.key_ref()[overlap..])
        }
        self.data.put_u64(key.ts());
        self.data.put_u16(value.len() as u16);
        self.data.put(value);
        true
    }

    /// Check if there is no key-value pair in the block.
    pub fn is_empty(&self) -> bool {
        self.first_key.is_empty()
    }

    /// Finalize the block.
    pub fn build(self) -> Block {
        Block {
            data: self.data,
            offsets: self.offsets,
        }
    }

    pub fn get_estimated_size(&self) -> usize {
        2 + self.offsets.len() * U16_SIZE + self.data.len()
    }
}
