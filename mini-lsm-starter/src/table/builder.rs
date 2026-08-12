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

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use bytes::BufMut;
use nom::AsBytes;

use super::{BlockMeta, SsTable};
use crate::{
    block::BlockBuilder,
    key::KeySlice,
    lsm_storage::BlockCache,
    table::{FileObject, bloom::Bloom},
};

/// Builds an SSTable from key-value pairs.
pub struct SsTableBuilder {
    builder: BlockBuilder,
    first_key: Vec<u8>,
    last_key: Vec<u8>,
    data: Vec<u8>,
    pub(crate) meta: Vec<BlockMeta>,
    key_hashes: Vec<u32>,
    block_size: usize,
    bloom_false_positive_rate: f64,
}

impl SsTableBuilder {
    /// Create a builder based on target block size.
    pub fn new(block_size: usize) -> Self {
        SsTableBuilder {
            builder: BlockBuilder::new(block_size),
            first_key: Vec::new(),
            last_key: Vec::new(),
            data: Vec::new(),
            meta: Vec::new(),
            key_hashes: Vec::new(),
            block_size,
            bloom_false_positive_rate: 0.01,
        }
    }

    fn finalize_block(&mut self) {
        let builder = std::mem::replace(&mut self.builder, BlockBuilder::new(self.block_size));

        let block = builder.build();
        self.meta.push(BlockMeta {
            offset: self.data.len(),
            first_key: block.get_first_key().into_key_bytes(),
            last_key: block.get_last_key().into_key_bytes(),
        });

        let block_data = block.encode();
        let crc = crc32fast::hash(block_data.as_bytes());
        self.data.extend(block_data);
        self.data.put_u32(crc);
    }

    /// Adds a key-value pair to SSTable.
    ///
    /// Note: You should split a new block when the current block is full.(`std::mem::replace` may
    /// be helpful here)
    pub fn add(&mut self, key: KeySlice, value: &[u8]) {
        if !self.builder.add(key, value) {
            self.finalize_block();
            _ = self.builder.add(key, value);
        }
        self.key_hashes.push(farmhash::fingerprint32(key.key_ref()));
    }

    /// Get the estimated size of the SSTable.
    ///
    /// Since the data blocks contain much more data than meta blocks, just return the size of data
    /// blocks here.
    pub fn estimated_size(&self) -> usize {
        self.data.len() + self.builder.get_estimated_size()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty() && self.builder.is_empty()
    }

    /// Builds the SSTable and writes it to the given path. Use the `FileObject` structure to manipulate the disk objects.
    pub fn build(
        #[allow(unused_mut)] mut self,
        id: usize,
        block_cache: Option<Arc<BlockCache>>,
        path: impl AsRef<Path>,
    ) -> Result<SsTable> {
        if !self.builder.is_empty() {
            self.finalize_block();
        }

        let meta_offset = self.data.len();
        BlockMeta::encode_block_meta(self.meta.as_slice(), &mut self.data);
        let crc = crc32fast::hash(&self.data[meta_offset..]);
        self.data.put_u32(crc);
        self.data.put_u32(meta_offset as u32);

        let bits_per_key =
            Bloom::bloom_bits_per_key(self.key_hashes.len(), self.bloom_false_positive_rate);
        let bloom = Bloom::build_from_key_hashes(self.key_hashes.as_slice(), bits_per_key);
        let bloom_offset = self.data.len();
        bloom.encode(&mut self.data);
        self.data.put_u32(bloom_offset as u32);

        let file_object = FileObject::create(path.as_ref(), self.data)?;
        let first_key = self.meta[0].first_key.clone();
        let last_key = self.meta[self.meta.len() - 1].last_key.clone();

        Ok(SsTable {
            file: file_object,
            block_meta: self.meta,
            block_meta_offset: meta_offset,
            id,
            block_cache,
            first_key,
            last_key,
            bloom: Some(bloom),
            max_ts: 0,
        })
    }

    #[cfg(test)]
    pub(crate) fn build_for_test(self, path: impl AsRef<Path>) -> Result<SsTable> {
        self.build(0, None, path)
    }
}
