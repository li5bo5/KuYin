use std::time::{SystemTime, UNIX_EPOCH};

use super::model::{PlaybackMode, Track};

pub struct PlaybackQueue {
    tracks: Vec<Track>,
    current_index: Option<usize>,
    mode: PlaybackMode,
    shuffle_indices: Vec<usize>,
    shuffle_pos: usize,
    pending_next_count: usize,
}

impl PlaybackQueue {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            current_index: None,
            mode: PlaybackMode::Sequence,
            shuffle_indices: Vec::new(),
            shuffle_pos: 0,
            pending_next_count: 0,
        }
    }

    pub fn set_mode(&mut self, mode: PlaybackMode) {
        self.mode = mode;
        if mode == PlaybackMode::Shuffle {
            self.reshuffle();
        }
    }

    pub fn get_mode(&self) -> PlaybackMode {
        self.mode
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn get_current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn set_current_index(&mut self, index: Option<usize>) {
        self.pending_next_count = 0;
        if let Some(idx) = index {
            if idx < self.tracks.len() {
                self.current_index = Some(idx);
                if self.mode == PlaybackMode::Shuffle {
                    self.ensure_shuffle_consistent();
                }
            }
        } else {
            self.current_index = None;
        }
    }

    pub fn get_current_track(&self) -> Option<Track> {
        self.current_index
            .and_then(|idx| self.tracks.get(idx).cloned())
    }

    pub fn get_tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// 单曲立即播放：若已在队列中则直接定位到对应索引，否则插入到当前播放曲目之后并定位（保留随机序列上下文）
    pub fn select_or_insert(&mut self, track: Track) -> usize {
        self.pending_next_count = 0;
        if let Some(pos) = self
            .tracks
            .iter()
            .position(|t| t.hash.eq_ignore_ascii_case(&track.hash))
        {
            self.current_index = Some(pos);
            if self.mode == PlaybackMode::Shuffle {
                self.ensure_shuffle_consistent();
            }
            return pos;
        }

        if self.mode == PlaybackMode::Shuffle {
            self.ensure_shuffle_consistent();
        }

        let insert_idx = match self.current_index {
            Some(curr) if curr < self.tracks.len() => curr + 1,
            _ => self.tracks.len(),
        };
        self.tracks.insert(insert_idx, track);
        self.current_index = Some(insert_idx);
        if self.mode == PlaybackMode::Shuffle {
            for x in &mut self.shuffle_indices {
                if *x >= insert_idx {
                    *x += 1;
                }
            }
            let target_s_pos = if self.shuffle_indices.is_empty() {
                0
            } else {
                (self.shuffle_pos + 1).min(self.shuffle_indices.len())
            };
            self.shuffle_indices.insert(target_s_pos, insert_idx);
            self.shuffle_pos = target_s_pos;
        }
        insert_idx
    }

    pub fn set_queue(&mut self, new_tracks: Vec<Track>, start_index: Option<usize>) {
        self.tracks = new_tracks;
        self.pending_next_count = 0;
        self.current_index = match start_index {
            Some(idx) if idx < self.tracks.len() => Some(idx),
            _ if !self.tracks.is_empty() => Some(0),
            _ => None,
        };
        if self.mode == PlaybackMode::Shuffle {
            self.reshuffle();
        }
    }

    /// 单曲「下一首播放」：仅把该歌曲插入到当前播放歌曲之后，不切歌；在随机/单曲循环下确保下一首优先播放
    pub fn insert_next(&mut self, track: Track) {
        if self.mode == PlaybackMode::Shuffle {
            self.ensure_shuffle_consistent();
        }

        if let Some(existing) = self
            .tracks
            .iter()
            .position(|t| t.hash.eq_ignore_ascii_case(&track.hash))
        {
            if Some(existing) == self.current_index {
                return;
            }
            self.remove_index_internal(existing);
        }

        if let Some(idx) = self.current_index {
            let target = (idx + 1).min(self.tracks.len());
            self.tracks.insert(target, track);
            self.pending_next_count += 1;
            if self.mode == PlaybackMode::Shuffle {
                for x in &mut self.shuffle_indices {
                    if *x >= target {
                        *x += 1;
                    }
                }
                let s_target = (self.shuffle_pos + 1).min(self.shuffle_indices.len());
                self.shuffle_indices.insert(s_target, target);
            }
        } else {
            self.tracks.push(track);
            self.current_index = Some(0);
            if self.mode == PlaybackMode::Shuffle {
                self.shuffle_indices = vec![0];
                self.shuffle_pos = 0;
            }
        }
    }

    /// 单曲「添加到队列」：仅追加到队列末尾，不切歌
    pub fn append_one(&mut self, track: Track) {
        if self.mode == PlaybackMode::Shuffle {
            self.ensure_shuffle_consistent();
        }

        if let Some(existing) = self
            .tracks
            .iter()
            .position(|t| t.hash.eq_ignore_ascii_case(&track.hash))
        {
            if Some(existing) == self.current_index {
                return;
            }
            self.remove_index_internal(existing);
        }

        let new_idx = self.tracks.len();
        self.tracks.push(track);
        if self.current_index.is_none() {
            self.current_index = Some(0);
        }
        if self.mode == PlaybackMode::Shuffle {
            self.shuffle_indices.push(new_idx);
        }
    }

    /// 歌单/列表「添加队列」：将所有歌曲追加到队列末尾，不切歌，返回添加曲目数
    pub fn append_tracks(&mut self, new_tracks: Vec<Track>) -> usize {
        if new_tracks.is_empty() {
            return 0;
        }
        let total = new_tracks.len();
        let was_empty = self.tracks.is_empty();
        for t in new_tracks {
            self.append_one(t);
        }
        if self.mode == PlaybackMode::Shuffle && self.shuffle_indices.len() > 1 {
            let tail_len = total.min(self.shuffle_indices.len());
            let start = if was_empty {
                1
            } else {
                self.shuffle_indices
                    .len()
                    .saturating_sub(tail_len)
                    .max(self.shuffle_pos + 1 + self.pending_next_count)
            };
            if start + 1 < self.shuffle_indices.len() {
                let mut seed = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0x9e3779b97f4a7c15);
                for i in ((start + 1)..self.shuffle_indices.len()).rev() {
                    seed = seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let j = start + ((seed as usize) % (i - start + 1));
                    self.shuffle_indices.swap(i, j);
                }
            }
        }
        total
    }

    fn remove_index_internal(&mut self, index: usize) -> Option<Track> {
        if index >= self.tracks.len() {
            return None;
        }
        let removed = self.tracks.remove(index);
        if let Some(curr) = self.current_index {
            if curr == index {
                if self.tracks.is_empty() {
                    self.current_index = None;
                } else if curr >= self.tracks.len() {
                    self.current_index = Some(0);
                }
            } else if curr > index {
                self.current_index = Some(curr - 1);
            }
        }
        if self.mode == PlaybackMode::Shuffle {
            if let Some(s_idx) = self.shuffle_indices.iter().position(|&x| x == index) {
                self.shuffle_indices.remove(s_idx);
                if s_idx < self.shuffle_pos {
                    self.shuffle_pos = self.shuffle_pos.saturating_sub(1);
                } else if self.shuffle_pos >= self.shuffle_indices.len()
                    && !self.shuffle_indices.is_empty()
                {
                    self.shuffle_pos = 0;
                }
            }
            for x in &mut self.shuffle_indices {
                if *x > index {
                    *x -= 1;
                }
            }
        }
        Some(removed)
    }

    pub fn remove_at(&mut self, index: usize) -> Option<Track> {
        if self.mode == PlaybackMode::Shuffle {
            self.ensure_shuffle_consistent();
        }
        self.remove_index_internal(index)
    }

    pub fn clear(&mut self) {
        self.tracks.clear();
        self.current_index = None;
        self.shuffle_indices.clear();
        self.shuffle_pos = 0;
        self.pending_next_count = 0;
    }

    /// 调整队列中歌曲顺序并同步修正当前播放索引
    pub fn move_item(&mut self, from: usize, to: usize) {
        let len = self.tracks.len();
        if from >= len || to >= len || from == to {
            return;
        }
        if self.mode == PlaybackMode::Shuffle {
            self.ensure_shuffle_consistent();
        }
        let item = self.tracks.remove(from);
        self.tracks.insert(to, item);

        if let Some(curr) = self.current_index {
            if curr == from {
                self.current_index = Some(to);
            } else if from < curr && curr <= to {
                self.current_index = Some(curr - 1);
            } else if to <= curr && curr < from {
                self.current_index = Some(curr + 1);
            }
        }

        if self.mode == PlaybackMode::Shuffle {
            for x in &mut self.shuffle_indices {
                if *x == from {
                    *x = to;
                } else if from < *x && *x <= to {
                    *x -= 1;
                } else if to <= *x && *x < from {
                    *x += 1;
                }
            }
        }
    }

    /// 更新队列中指定曲目的封面 URL（用于异步元数据补齐）
    pub fn update_track_cover(&mut self, hash: &str, cover_url: &str) {
        for t in &mut self.tracks {
            if t.hash.eq_ignore_ascii_case(hash) && t.cover_url.as_deref().unwrap_or("").is_empty() {
                t.cover_url = Some(cover_url.to_string());
            }
        }
    }

    /// 自然播放结束时的下一首（若用户显式设置了「下一首播放」则优先消费；单曲循环返回当前曲目；顺序播放到队尾停止）
    pub fn next_on_ended(&mut self) -> Option<Track> {
        if self.tracks.is_empty() {
            return None;
        }
        if self.pending_next_count > 0 {
            self.pending_next_count -= 1;
            return self.advance_next(true);
        }
        if self.mode == PlaybackMode::Single {
            return self.get_current_track();
        }
        self.advance_next(self.mode == PlaybackMode::Loop)
    }

    /// 当前曲目全部品质重试后仍无法播放时，按当前播放规则跳过至下一首（避免单曲循环原地死锁）
    pub fn next_on_skip(&mut self) -> Option<Track> {
        if self.tracks.len() <= 1 {
            return None;
        }
        self.pending_next_count = self.pending_next_count.saturating_sub(1);
        let wrap = matches!(self.mode, PlaybackMode::Loop | PlaybackMode::Single | PlaybackMode::Shuffle);
        self.advance_next(wrap)
    }

    /// 用户手动点击「下一首」（即使单曲循环也切换到下一首）
    pub fn next_manual(&mut self) -> Option<Track> {
        if self.tracks.is_empty() {
            return None;
        }
        self.pending_next_count = self.pending_next_count.saturating_sub(1);
        self.advance_next(true)
    }

    fn advance_next(&mut self, wrap_around: bool) -> Option<Track> {
        if self.mode == PlaybackMode::Shuffle {
            self.ensure_shuffle_consistent();
            if self.shuffle_indices.is_empty() {
                return None;
            }
            self.shuffle_pos = (self.shuffle_pos + 1) % self.shuffle_indices.len();
            let next_idx = self.shuffle_indices[self.shuffle_pos];
            self.current_index = Some(next_idx);
            return self.tracks.get(next_idx).cloned();
        }

        let next_idx = match self.current_index {
            Some(curr) => {
                if curr + 1 < self.tracks.len() {
                    curr + 1
                } else if wrap_around {
                    0
                } else {
                    return None;
                }
            }
            None => 0,
        };
        self.current_index = Some(next_idx);
        self.tracks.get(next_idx).cloned()
    }

    pub fn previous_track(&mut self) -> Option<Track> {
        if self.tracks.is_empty() {
            return None;
        }
        self.pending_next_count = 0;

        if self.mode == PlaybackMode::Shuffle {
            self.ensure_shuffle_consistent();
            self.shuffle_pos = if self.shuffle_pos == 0 {
                self.shuffle_indices.len().saturating_sub(1)
            } else {
                self.shuffle_pos - 1
            };
            let prev_idx = self.shuffle_indices[self.shuffle_pos];
            self.current_index = Some(prev_idx);
            return self.tracks.get(prev_idx).cloned();
        }

        let prev_idx = match self.current_index {
            Some(curr) => {
                if curr == 0 {
                    self.tracks.len() - 1
                } else {
                    curr - 1
                }
            }
            None => 0,
        };
        self.current_index = Some(prev_idx);
        self.tracks.get(prev_idx).cloned()
    }

    fn ensure_shuffle_consistent(&mut self) {
        if self.tracks.is_empty() {
            self.shuffle_indices.clear();
            self.shuffle_pos = 0;
            return;
        }
        if self.shuffle_indices.len() != self.tracks.len() {
            self.reshuffle();
            return;
        }
        if let Some(curr) = self.current_index {
            if self.shuffle_pos >= self.shuffle_indices.len()
                || self.shuffle_indices[self.shuffle_pos] != curr
            {
                if let Some(pos) = self.shuffle_indices.iter().position(|&x| x == curr) {
                    self.shuffle_pos = pos;
                } else {
                    self.reshuffle();
                }
            }
        }
    }

    fn reshuffle(&mut self) {
        let len = self.tracks.len();
        if len == 0 {
            self.shuffle_indices.clear();
            self.shuffle_pos = 0;
            return;
        }
        let mut indices: Vec<usize> = (0..len).collect();
        let mut seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e3779b97f4a7c15);

        // Fisher-Yates 随机洗牌（保留当前播放曲目在首位避免立即重复）
        for i in (1..len).rev() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let j = (seed as usize) % (i + 1);
            indices.swap(i, j);
        }

        if let Some(curr) = self.current_index {
            if let Some(pos) = indices.iter().position(|&x| x == curr) {
                indices.swap(0, pos);
            }
        }
        self.shuffle_indices = indices;
        self.shuffle_pos = 0;
    }
}
