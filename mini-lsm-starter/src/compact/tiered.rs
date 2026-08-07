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

#[derive(Debug, Serialize, Deserialize)]
pub struct TieredCompactionTask {
    pub tiers: Vec<(usize, Vec<usize>)>,
    pub bottom_tier_included: bool,
}

#[derive(Debug, Clone)]
pub struct TieredCompactionOptions {
    pub num_tiers: usize,
    pub max_size_amplification_percent: usize,
    pub size_ratio: usize,
    pub min_merge_width: usize,
    pub max_merge_width: Option<usize>,
}

pub struct TieredCompactionController {
    options: TieredCompactionOptions,
}

impl TieredCompactionController {
    pub fn new(options: TieredCompactionOptions) -> Self {
        Self { options }
    }

    pub fn generate_compaction_task(
        &self,
        snapshot: &LsmStorageState,
    ) -> Option<TieredCompactionTask> {
        if snapshot.levels.len() < self.options.num_tiers {
            return None;
        }
        let total_size: usize = snapshot.levels.iter().map(|(_, ids)| ids.len()).sum();
        let last_size = snapshot.levels.last().map_or(0, |(_, ids)| ids.len());
        if last_size * self.options.max_size_amplification_percent <= (total_size - last_size) * 100
        {
            return Some(TieredCompactionTask {
                tiers: snapshot.levels.clone(),
                bottom_tier_included: true,
            });
        }
        let mut cum_size: usize = snapshot
            .levels
            .iter()
            .take(self.options.min_merge_width)
            .map(|(_, ids)| ids.len())
            .sum();
        for i in self.options.min_merge_width..snapshot.levels.len() {
            let (level, ids) = snapshot.levels.get(i).unwrap();
            if ids.len() * 100 > cum_size * (100 + self.options.size_ratio) {
                return Some(TieredCompactionTask {
                    tiers: snapshot.levels.iter().take(i).cloned().collect(),
                    bottom_tier_included: false,
                });
            }
            cum_size += ids.len()
        }
        if let Some(width) = self.options.max_merge_width {
            Some(TieredCompactionTask {
                tiers: snapshot.levels.iter().take(width).cloned().collect(),
                bottom_tier_included: width >= snapshot.levels.len(),
            })
        } else {
            Some(TieredCompactionTask {
                tiers: snapshot.levels.clone(),
                bottom_tier_included: true,
            })
        }
    }

    pub fn apply_compaction_result(
        &self,
        snapshot: &LsmStorageState,
        task: &TieredCompactionTask,
        output: &[usize],
    ) -> (LsmStorageState, Vec<usize>) {
        assert!(!task.tiers.is_empty());
        let mut snapshot = snapshot.clone();

        let mut start = 0;
        for i in 0..snapshot.levels.len() {
            if snapshot.levels.get(i).unwrap().0 == task.tiers.first().unwrap().0 {
                start = i;
            }
        }

        snapshot.levels.drain(start..start + task.tiers.len() - 1);
        *snapshot.levels.get_mut(start).unwrap() = (*output.first().unwrap(), Vec::from(output));

        let to_remove = task
            .tiers
            .iter()
            .flat_map(|(_, ids)| ids)
            .copied()
            .collect();
        (snapshot, to_remove)
    }
}
