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

use serde::{Deserialize, Serialize};

use crate::lsm_storage::LsmStorageState;

#[derive(Debug, Clone)]
pub struct SimpleLeveledCompactionOptions {
    pub size_ratio_percent: usize,
    pub level0_file_num_compaction_trigger: usize,
    pub max_levels: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SimpleLeveledCompactionTask {
    // if upper_level is `None`, then it is L0 compaction
    pub upper_level: Option<usize>,
    pub upper_level_sst_ids: Vec<usize>,
    pub lower_level: usize,
    pub lower_level_sst_ids: Vec<usize>,
    pub is_lower_level_bottom_level: bool,
}

pub struct SimpleLeveledCompactionController {
    options: SimpleLeveledCompactionOptions,
}

impl SimpleLeveledCompactionController {
    pub fn new(options: SimpleLeveledCompactionOptions) -> Self {
        Self { options }
    }

    /// Generates a compaction task.
    ///
    /// Returns `None` if no compaction needs to be scheduled. The order of SSTs in the compaction task id vector matters.
    pub fn generate_compaction_task(
        &self,
        snapshot: &LsmStorageState,
    ) -> Option<SimpleLeveledCompactionTask> {
        if snapshot.l0_sstables.len() >= self.options.level0_file_num_compaction_trigger {
            return Some(SimpleLeveledCompactionTask {
                upper_level: None,
                upper_level_sst_ids: snapshot.l0_sstables.clone(),
                lower_level: 1,
                lower_level_sst_ids: snapshot
                    .levels
                    .first()
                    .map(|(_, ids)| ids.clone())
                    .unwrap_or_else(Vec::new),
                is_lower_level_bottom_level: self.options.max_levels == 1,
            });
        }
        for (level, ids) in snapshot.levels.iter() {
            if *level == self.options.max_levels {
                break;
            }
            let next_level = level + 1;
            let next_level_ids = snapshot.levels.get(next_level - 1);
            let next_level_count = next_level_ids.map_or(0, |(_, ids)| ids.len());
            if ids.len() * self.options.size_ratio_percent / 100 > next_level_count {
                return Some(SimpleLeveledCompactionTask {
                    upper_level: Some(*level),
                    upper_level_sst_ids: ids.clone(),
                    lower_level: next_level,
                    lower_level_sst_ids: next_level_ids
                        .map(|(_, ids)| ids.clone())
                        .unwrap_or_else(Vec::new),
                    is_lower_level_bottom_level: next_level == self.options.max_levels,
                });
            }
        }
        None
    }

    /// Apply the compaction result.
    ///
    /// The compactor will call this function with the compaction task and the list of SST ids generated. This function applies the
    /// result and generates a new LSM state. The functions should only change `l0_sstables` and `levels` without changing memtables
    /// and `sstables` hash map. Though there should only be one thread running compaction jobs, you should think about the case
    /// where an L0 SST gets flushed while the compactor generates new SSTs, and with that in mind, you should do some sanity checks
    /// in your implementation.
    pub fn apply_compaction_result(
        &self,
        snapshot: &LsmStorageState,
        task: &SimpleLeveledCompactionTask,
        output: &[usize],
    ) -> (LsmStorageState, Vec<usize>) {
        let mut snapshot = snapshot.clone();

        if let Some(upper_level) = task.upper_level {
            assert!(snapshot.levels.len() >= upper_level);
            snapshot.levels.get_mut(upper_level - 1).unwrap().1.clear();
        } else {
            assert!(!task.upper_level_sst_ids.is_empty());
            for i in 0..snapshot.l0_sstables.len() {
                if snapshot.l0_sstables[i] == *task.upper_level_sst_ids.first().unwrap() {
                    snapshot.l0_sstables.resize(i, 0);
                    break;
                }
            }
        }

        if let Some((level, ids)) = snapshot.levels.get_mut(task.lower_level - 1) {
            ids.clear();
            ids.extend_from_slice(output);
        } else {
            snapshot.levels.push((task.lower_level, Vec::from(output)));
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
