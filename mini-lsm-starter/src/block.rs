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

mod builder;
mod iterator;

const U16_SIZE: usize = std::mem::size_of::<u16>();

pub use builder::BlockBuilder;
use bytes::{Buf, BufMut, Bytes};
pub use iterator::BlockIterator;

use crate::key::KeySlice;

/// A block is the smallest unit of read and caching in LSM tree. It is a collection of sorted key-value pairs.
pub struct Block {
    pub(crate) data: Vec<u8>,
    pub(crate) offsets: Vec<u16>,
}

fn read_value<'slice>(buf: &mut &'slice [u8]) -> &'slice [u8] {
    let len = buf.get_u16() as usize;
    let result = &buf[..len];
    buf.advance(len);
    result
}

impl Block {
    /// Encode the internal data to the data layout illustrated in the course
    /// Note: You may want to recheck if any of the expected field is missing from your output
    pub fn encode(&self) -> Bytes {
        let mut data = Vec::with_capacity(self.data.len() + (self.offsets.len() + 1) * U16_SIZE);
        data.extend(self.data.iter());
        self.offsets.iter().for_each(|offset| data.put_u16(*offset));
        data.put_u16(self.offsets.len() as u16);
        Bytes::from(data)
    }

    /// Decode from the data layout, transform the input `data` to a single `Block`
    pub fn decode(data: &[u8]) -> Self {
        let count_offset = data.len() - U16_SIZE;
        let count = (&data[count_offset..]).get_u16() as usize;
        let offsets_offset = data.len() - U16_SIZE * (count + 1);
        let mut offsets = Vec::with_capacity(count);
        let mut raw_offsets = &data[offsets_offset..count_offset];
        while let Ok(offset) = raw_offsets.try_get_u16() {
            offsets.push(offset);
        }
        Self {
            data: Vec::from(&data[..offsets_offset]),
            offsets,
        }
    }

    pub fn count(&self) -> usize {
        self.offsets.len()
    }

    pub fn get_entry(&self, idx: usize) -> (KeySlice<'_>, (usize, usize)) {
        let offset = self.offsets.get(idx);
        if let Some(offset) = offset {
            let offset = *offset as usize;
            let mut entry_buf = &self.data[offset..];
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

    pub fn find_key(&self, key: KeySlice) -> usize {
        self.offsets.partition_point(|offset| {
            let mut buf = &self.data[(*offset as usize)..];
            read_value(&mut buf) < key.raw_ref()
        })
    }

    pub fn get_first_key(&self) -> KeySlice<'_> {
        self.get_entry(0).0
    }

    pub fn get_last_key(&self) -> KeySlice<'_> {
        self.get_entry(self.count() - 1).0
    }
}
