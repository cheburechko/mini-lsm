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

use anyhow::Result;

use serde::{Deserialize, Serialize};

use crate::lsm_storage::{LsmStorageState, has_intersection};

#[derive(Debug, Serialize, Deserialize)]
pub struct LeveledCompactionTask {
    // if upper_level is `None`, then it is L0 compaction
    pub upper_level: Option<usize>,
    pub upper_level_sst_ids: Vec<usize>,
    pub lower_level: usize,
    pub lower_level_sst_ids: Vec<usize>,
    pub is_lower_level_bottom_level: bool,
}

#[derive(Debug, Clone)]
pub struct LeveledCompactionOptions {
    pub level_size_multiplier: usize,
    pub level0_file_num_compaction_trigger: usize,
    pub max_levels: usize,
    pub base_level_size_mb: usize,
}

pub struct LeveledCompactionController {
    options: LeveledCompactionOptions,
}

impl LeveledCompactionController {
    pub fn new(options: LeveledCompactionOptions) -> Self {
        Self { options }
    }

    fn find_overlapping_ssts(
        &self,
        snapshot: &LsmStorageState,
        sst_ids: &[usize],
        in_level: usize,
    ) -> Vec<usize> {
        let tables = snapshot.get_sstables(sst_ids).unwrap();
        let lower = std::ops::Bound::Included(
            tables
                .iter()
                .map(|table| table.first_key().raw_ref())
                .min_by_key(|x| *x)
                .unwrap(),
        );
        let upper = std::ops::Bound::Included(
            tables
                .iter()
                .map(|table| table.last_key().raw_ref())
                .max_by_key(|x| *x)
                .unwrap(),
        );
        snapshot
            .levels
            .get(in_level - 1)
            .unwrap()
            .1
            .iter()
            .filter(|id| has_intersection(snapshot.sstables.get(id).unwrap(), lower, upper))
            .copied()
            .collect()
    }

    fn calculate_size(snapshot: &LsmStorageState, ids: &[usize]) -> Result<usize> {
        let tables = snapshot.get_sstables(ids)?;
        Ok(tables.iter().map(|table| table.table_size()).sum::<u64>() as usize)
    }

    fn calculate_target_sizes(&self, snapshot: &LsmStorageState) -> Result<Vec<usize>> {
        let mut sizes = vec![0; self.options.max_levels];

        let base_size = 1024 * 1024 * self.options.base_level_size_mb;
        let mut target_size =
            if let Some((_, ids)) = snapshot.levels.get(self.options.max_levels - 1) {
                Self::calculate_size(snapshot, ids)
            } else {
                Ok(0)
            }?;

        if target_size < base_size {
            *sizes.last_mut().unwrap() = base_size;
            return Ok(sizes);
        }

        for cur_size in sizes.iter_mut().rev() {
            *cur_size = target_size;
            if target_size < base_size {
                break;
            }
            target_size /= self.options.level_size_multiplier;
        }

        Ok(sizes)
    }

    pub fn generate_compaction_task(
        &self,
        snapshot: &LsmStorageState,
    ) -> Option<LeveledCompactionTask> {
        let target_sizes = self.calculate_target_sizes(snapshot).unwrap();
        if snapshot.l0_sstables.len() >= self.options.level0_file_num_compaction_trigger {
            let level = target_sizes.partition_point(|x| *x == 0) + 1;
            return Some(LeveledCompactionTask {
                upper_level: None,
                upper_level_sst_ids: snapshot.l0_sstables.clone(),
                lower_level: level,
                lower_level_sst_ids: self.find_overlapping_ssts(snapshot, snapshot.l0_sstables.as_slice(), level),
                is_lower_level_bottom_level: level == self.options.max_levels,
            });
        }

        let mut max_level = 1;
        let mut max_priority = 0.0;
        for (level, ids) in snapshot.levels.iter().take(self.options.max_levels - 1) {
            let priority = if target_sizes[level - 1] == 0 {
                0.0
            } else {
                Self::calculate_size(snapshot, ids.as_slice()).unwrap() as f64
                    / target_sizes[level - 1] as f64
            };
            if priority > max_priority {
                max_priority = priority;
                max_level = *level;
            }
        }

        if max_priority > 1.0 {
            let upper_level_sst_ids = vec![
                *snapshot
                    .levels
                    .get(max_level - 1)
                    .unwrap()
                    .1
                    .iter()
                    .reduce(|a, b| a.min(b))
                    .unwrap(),
            ];
            let lower_level_sst_ids =
                self.find_overlapping_ssts(snapshot, upper_level_sst_ids.as_slice(), max_level + 1);
            return Some(LeveledCompactionTask {
                upper_level: Some(max_level),
                upper_level_sst_ids: upper_level_sst_ids,
                lower_level: max_level + 1,
                lower_level_sst_ids: lower_level_sst_ids,
                is_lower_level_bottom_level: max_level + 1 == self.options.max_levels,
            });
        }

        None
    }

    pub fn apply_compaction_result(
        &self,
        snapshot: &LsmStorageState,
        task: &LeveledCompactionTask,
        output: &[usize],
        _in_recovery: bool,
    ) -> (LsmStorageState, Vec<usize>) {
        let mut snapshot = snapshot.clone();

        if let Some(upper_level) = task.upper_level {
            let (level, ids) = snapshot.levels.get_mut(upper_level - 1).unwrap();
            assert_eq!(*level, upper_level);
            assert_eq!(task.upper_level_sst_ids.len(), 1);
            let upper_level_sst_id = task.upper_level_sst_ids.first().unwrap();
            ids.retain(|id| id != upper_level_sst_id);
        } else {
            let id = *task.upper_level_sst_ids.first().unwrap();

            for i in 0..snapshot.l0_sstables.len() {
                if snapshot.l0_sstables[i] == id {
                    snapshot.l0_sstables.truncate(i);
                    break;
                }
            }
        }

        while snapshot.levels.len() < self.options.max_levels {
            snapshot
                .levels
                .push((snapshot.levels.len() + 1, Vec::new()));
        }

        let (level, ids) = snapshot.levels.get_mut(task.lower_level - 1).unwrap();
        assert_eq!(level, &task.lower_level);
        let before = ids.len();
        let mut start = None;
        let mut end = 0;
        if !ids.is_empty() {
            let lower = snapshot
                .sstables
                .get(output.first().unwrap())
                .unwrap()
                .first_key();
            let upper = snapshot
                .sstables
                .get(output.last().unwrap())
                .unwrap()
                .last_key();
            start = {
                let pos =
                    ids.partition_point(|id| snapshot.sstables.get(id).unwrap().last_key() < lower);
                if pos == ids.len() { None } else { Some(pos) }
            };
            end = ids
                .partition_point(|id| snapshot.sstables.get(id).unwrap().first_key() < upper)
        }

        if let Some(start) = start {
            ids.splice(start..end, output.iter().copied());
        } else {
            ids.extend_from_slice(output);
        }

        let to_remove = task
            .lower_level_sst_ids
            .iter()
            .chain(task.upper_level_sst_ids.iter())
            .copied()
            .collect();
        (snapshot, to_remove)
    }
}
