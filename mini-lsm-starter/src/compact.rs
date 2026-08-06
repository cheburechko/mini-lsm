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

mod leveled;
mod simple_leveled;
mod tiered;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
pub use leveled::{LeveledCompactionController, LeveledCompactionOptions, LeveledCompactionTask};
use serde::{Deserialize, Serialize};
pub use simple_leveled::{
    SimpleLeveledCompactionController, SimpleLeveledCompactionOptions, SimpleLeveledCompactionTask,
};
pub use tiered::{TieredCompactionController, TieredCompactionOptions, TieredCompactionTask};

use crate::iterators::StorageIterator;
use crate::iterators::concat_iterator::SstConcatIterator;
use crate::iterators::merge_iterator::MergeIterator;
use crate::iterators::two_merge_iterator::TwoMergeIterator;
use crate::key::Key;
use crate::lsm_storage::{LsmStorageInner, LsmStorageState};
use crate::table::{SsTable, SsTableBuilder, SsTableIterator};

#[derive(Debug, Serialize, Deserialize)]
pub enum CompactionTask {
    Leveled(LeveledCompactionTask),
    Tiered(TieredCompactionTask),
    Simple(SimpleLeveledCompactionTask),
    ForceFullCompaction {
        l0_sstables: Vec<usize>,
        l1_sstables: Vec<usize>,
    },
}

impl CompactionTask {
    fn compact_to_bottom_level(&self) -> bool {
        match self {
            CompactionTask::ForceFullCompaction { .. } => true,
            CompactionTask::Leveled(task) => task.is_lower_level_bottom_level,
            CompactionTask::Simple(task) => task.is_lower_level_bottom_level,
            CompactionTask::Tiered(task) => task.bottom_tier_included,
        }
    }
}

pub(crate) enum CompactionController {
    Leveled(LeveledCompactionController),
    Tiered(TieredCompactionController),
    Simple(SimpleLeveledCompactionController),
    NoCompaction,
}

impl CompactionController {
    pub fn generate_compaction_task(&self, snapshot: &LsmStorageState) -> Option<CompactionTask> {
        match self {
            CompactionController::Leveled(ctrl) => ctrl
                .generate_compaction_task(snapshot)
                .map(CompactionTask::Leveled),
            CompactionController::Simple(ctrl) => ctrl
                .generate_compaction_task(snapshot)
                .map(CompactionTask::Simple),
            CompactionController::Tiered(ctrl) => ctrl
                .generate_compaction_task(snapshot)
                .map(CompactionTask::Tiered),
            CompactionController::NoCompaction => unreachable!(),
        }
    }

    pub fn apply_compaction_result(
        &self,
        snapshot: &LsmStorageState,
        task: &CompactionTask,
        output: &[usize],
        in_recovery: bool,
    ) -> (LsmStorageState, Vec<usize>) {
        match (self, task) {
            (CompactionController::Leveled(ctrl), CompactionTask::Leveled(task)) => {
                ctrl.apply_compaction_result(snapshot, task, output, in_recovery)
            }
            (CompactionController::Simple(ctrl), CompactionTask::Simple(task)) => {
                ctrl.apply_compaction_result(snapshot, task, output)
            }
            (CompactionController::Tiered(ctrl), CompactionTask::Tiered(task)) => {
                ctrl.apply_compaction_result(snapshot, task, output)
            }
            _ => unreachable!(),
        }
    }
}

impl CompactionController {
    pub fn flush_to_l0(&self) -> bool {
        matches!(
            self,
            Self::Leveled(_) | Self::Simple(_) | Self::NoCompaction
        )
    }
}

#[derive(Debug, Clone)]
pub enum CompactionOptions {
    /// Leveled compaction with partial compaction + dynamic level support (= RocksDB's Leveled
    /// Compaction)
    Leveled(LeveledCompactionOptions),
    /// Tiered compaction (= RocksDB's universal compaction)
    Tiered(TieredCompactionOptions),
    /// Simple leveled compaction
    Simple(SimpleLeveledCompactionOptions),
    /// In no compaction mode (week 1), always flush to L0
    NoCompaction,
}

impl LsmStorageInner {
    fn compact(&self, task: &CompactionTask) -> Result<Vec<Arc<SsTable>>> {
        match task {
            CompactionTask::ForceFullCompaction {
                l0_sstables,
                l1_sstables,
            } => {
                let guard = self.state.read();

                let l0_tables = guard.get_sstables(l0_sstables)?;
                let l1_tables = guard.get_sstables(l1_sstables)?;

                drop(guard);

                let l0_iter = Self::create_l0_iter(l0_tables)?;
                let l1_iter = SstConcatIterator::create_and_seek_to_first(l1_tables)?;
                let iter = TwoMergeIterator::create(l0_iter, l1_iter)?;
                self.build(iter, true)
            }
            CompactionTask::Simple(task) => {
                let guard = self.state.read();

                let upper = guard.get_sstables(&task.upper_level_sst_ids)?;
                let lower = guard.get_sstables(&task.lower_level_sst_ids)?;

                drop(guard);

                let lower_iter = SstConcatIterator::create_and_seek_to_first(lower)?;

                if let Some(_) = task.upper_level {
                    self.build(
                        TwoMergeIterator::create(
                            SstConcatIterator::create_and_seek_to_first(upper)?,
                            lower_iter,
                        )?,
                        task.is_lower_level_bottom_level,
                    )
                } else {
                    self.build(
                        TwoMergeIterator::create(Self::create_l0_iter(upper)?, lower_iter)?,
                        task.is_lower_level_bottom_level,
                    )
                }
            }
            _ => Err(anyhow::anyhow!("task not implemented: {:?}", task)),
        }
    }

    fn create_l0_iter(tables: Vec<Arc<SsTable>>) -> Result<MergeIterator<SsTableIterator>> {
        Ok(MergeIterator::create(
            tables
                .into_iter()
                .map(|table| Ok(Box::new(SsTableIterator::create_and_seek_to_first(table)?)))
                .collect::<Result<Vec<_>>>()?,
        ))
    }

    fn build<I: 'static + for<'a> StorageIterator<KeyType<'a> = Key<&'a [u8]>>>(
        &self,
        mut iter: I,
        is_final: bool,
    ) -> Result<Vec<Arc<SsTable>>> {
        let mut result = Vec::new();

        let mut builder = SsTableBuilder::new(self.options.block_size);
        let mut build = |builder: SsTableBuilder| -> Result<SsTableBuilder> {
            let id = self.next_sst_id();
            result.push(Arc::new(builder.build(
                id,
                Some(Arc::clone(&self.block_cache)),
                self.path_of_sst(id),
            )?));
            Ok(SsTableBuilder::new(self.options.block_size))
        };

        while iter.is_valid() {
            if !is_final || !iter.value().is_empty() {
                builder.add(iter.key(), iter.value());
            }
            if builder.estimated_size() >= self.options.target_sst_size {
                builder = build(builder)?;
            }
            iter.next()?;
        }
        if builder.estimated_size() > 0 {
            build(builder)?;
        }

        Ok(result)
    }

    pub fn force_full_compaction(&self) -> Result<()> {
        let task = {
            let guard = self.state.read();
            if guard.l0_sstables.is_empty() {
                return Ok(());
            }
            let l1 = if let Some((_, l1)) = guard.levels.first() {
                l1.clone()
            } else {
                Vec::new()
            };
            CompactionTask::ForceFullCompaction {
                l0_sstables: guard.l0_sstables.clone(),
                l1_sstables: l1,
            }
        };
        let l1 = self.compact(&task)?;
        let l1_ids = l1.iter().map(|table| table.sst_id()).collect();

        if let CompactionTask::ForceFullCompaction {
            l0_sstables,
            l1_sstables,
        } = task
        {
            {
                let lock = self.state_lock.lock();
                let mut guard = self.state.write();
                let state = Arc::get_mut(&mut guard).context("failed to get state")?;

                for i in 0..state.l0_sstables.len() {
                    if state.l0_sstables.get(i) == l0_sstables.first() {
                        state.l0_sstables.resize(i, 0);
                        break;
                    }
                }
                if !state.levels.is_empty() {
                    state.levels[0].1 = l1_ids;
                } else {
                    state.levels.push((1, l1_ids));
                }

                for id in l0_sstables.iter().chain(l1_sstables.iter()) {
                    state.sstables.remove(id);
                }
                for table in l1 {
                    state.sstables.insert(table.sst_id(), table);
                }
            }
            for id in l0_sstables.into_iter().chain(l1_sstables) {
                std::fs::remove_file(self.path_of_sst(id))?;
            }
            Ok(())
        } else {
            unreachable!();
        }
    }

    fn trigger_compaction(&self) -> Result<()> {
        let task = {
            let guard = self.state.read();
            self.compaction_controller
                .generate_compaction_task(guard.as_ref())
        };

        if let Some(task) = task {
            let tables = self.compact(&task)?;

            let to_remove = {
                let lock = self.state_lock.lock();
                let mut guard = self.state.write();
                let state = Arc::get_mut(&mut guard).context("failed to get state")?;
                let output: Vec<_> = tables.iter().map(|table| table.sst_id()).collect();
                let (mut new_state, to_remove) = self
                    .compaction_controller
                    .apply_compaction_result(state, &task, output.as_ref(), false);
                for table in tables {
                    new_state.sstables.insert(table.sst_id(), table);
                }
                for id in to_remove.iter() {
                    new_state.sstables.remove(id);
                }
                *state = new_state;
                to_remove
            };

            for id in to_remove {
                std::fs::remove_file(self.path_of_sst(id))?;
            }
        }
        Ok(())
    }

    pub(crate) fn spawn_compaction_thread(
        self: &Arc<Self>,
        rx: crossbeam_channel::Receiver<()>,
    ) -> Result<Option<std::thread::JoinHandle<()>>> {
        if let CompactionOptions::Leveled(_)
        | CompactionOptions::Simple(_)
        | CompactionOptions::Tiered(_) = self.options.compaction_options
        {
            let this = self.clone();
            let handle = std::thread::spawn(move || {
                let ticker = crossbeam_channel::tick(Duration::from_millis(50));
                loop {
                    crossbeam_channel::select! {
                        recv(ticker) -> _ => if let Err(e) = this.trigger_compaction() {
                            eprintln!("compaction failed: {}", e);
                        },
                        recv(rx) -> _ => return
                    }
                }
            });
            return Ok(Some(handle));
        }
        Ok(None)
    }

    fn trigger_flush(&self) -> Result<()> {
        if self.state.read().imm_memtables.len() >= self.options.num_memtable_limit {
            self.force_flush_next_imm_memtable()?;
        }
        Ok(())
    }

    pub(crate) fn spawn_flush_thread(
        self: &Arc<Self>,
        rx: crossbeam_channel::Receiver<()>,
    ) -> Result<Option<std::thread::JoinHandle<()>>> {
        let this = self.clone();
        let handle = std::thread::spawn(move || {
            let ticker = crossbeam_channel::tick(Duration::from_millis(50));
            loop {
                crossbeam_channel::select! {
                    recv(ticker) -> _ => if let Err(e) = this.trigger_flush() {
                        eprintln!("flush failed: {}", e);
                    },
                    recv(rx) -> _ => return
                }
            }
        });
        Ok(Some(handle))
    }
}
