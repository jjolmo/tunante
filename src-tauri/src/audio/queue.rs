use crate::db::models::Track;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

pub struct PlayQueue {
    tracks: Vec<Track>,
    current_index: Option<usize>,
    shuffle: bool,
    repeat: RepeatMode,
    shuffle_order: Vec<usize>,
    user_queue: Vec<Track>,
    continue_from_queue: bool,
    /// When a queued track is not found in the current context, store it here
    /// so the caller (event loop) can update the context from the DB.
    pending_context_update: Option<Track>,
}

impl PlayQueue {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            current_index: None,
            shuffle: false,
            repeat: RepeatMode::Off,
            shuffle_order: Vec::new(),
            user_queue: Vec::new(),
            continue_from_queue: true,
            pending_context_update: None,
        }
    }

    pub fn set_tracks(&mut self, tracks: Vec<Track>) {
        self.tracks = tracks;
        self.current_index = None;
        self.regenerate_shuffle();
    }

    pub fn play_index(&mut self, index: usize) -> Option<&Track> {
        if index < self.tracks.len() {
            self.current_index = Some(index);
            Some(&self.tracks[index])
        } else {
            None
        }
    }

    pub fn play_track_by_id(&mut self, id: &str) -> Option<&Track> {
        if let Some(idx) = self.tracks.iter().position(|t| t.id == id) {
            self.current_index = Some(idx);
            Some(&self.tracks[idx])
        } else {
            None
        }
    }

    pub fn current(&self) -> Option<&Track> {
        self.current_index.and_then(|i| self.tracks.get(i))
    }

    pub fn next(&mut self) -> Option<Track> {
        // User queue takes priority
        if !self.user_queue.is_empty() {
            let track = self.user_queue.remove(0);
            if self.continue_from_queue {
                // Try to find the queued track in the current context
                if let Some(idx) = self.tracks.iter().position(|t| t.id == track.id) {
                    self.current_index = Some(idx);
                    self.pending_context_update = None;
                } else {
                    // Track not in context — signal that caller should update context
                    self.pending_context_update = Some(track.clone());
                }
            }
            return Some(track);
        }

        self.pending_context_update = None;

        if self.tracks.is_empty() {
            return None;
        }

        match self.repeat {
            RepeatMode::One => {
                return self.current_index.map(|i| self.tracks[i].clone());
            }
            RepeatMode::All => {
                let next_idx = match self.current_index {
                    Some(i) => {
                        if self.shuffle {
                            self.next_shuffle_index(i).0
                        } else {
                            (i + 1) % self.tracks.len()
                        }
                    }
                    None => 0,
                };
                self.current_index = Some(next_idx);
                return Some(self.tracks[next_idx].clone());
            }
            RepeatMode::Off => {
                let next_idx = match self.current_index {
                    Some(i) => {
                        if self.shuffle {
                            let (ni, wrapped) = self.next_shuffle_index(i);
                            if wrapped {
                                return None;
                            }
                            ni
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                if next_idx < self.tracks.len() {
                    self.current_index = Some(next_idx);
                    return Some(self.tracks[next_idx].clone());
                }
                None
            }
        }
    }

    pub fn prev(&mut self) -> Option<Track> {
        if self.tracks.is_empty() {
            return None;
        }

        let prev_idx = match self.current_index {
            Some(i) if i > 0 => i - 1,
            Some(_) => {
                if self.repeat == RepeatMode::All {
                    self.tracks.len() - 1
                } else {
                    0
                }
            }
            None => 0,
        };

        self.current_index = Some(prev_idx);
        Some(self.tracks[prev_idx].clone())
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        self.shuffle = shuffle;
        if shuffle {
            self.regenerate_shuffle();
        }
    }

    pub fn set_repeat(&mut self, repeat: RepeatMode) {
        self.repeat = repeat;
    }

    fn regenerate_shuffle(&mut self) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let len = self.tracks.len();
        self.shuffle_order = (0..len).collect();

        // Simple Fisher-Yates shuffle using a hasher for pseudo-randomness
        let mut hasher = DefaultHasher::new();
        std::time::SystemTime::now().hash(&mut hasher);
        let mut seed = hasher.finish();

        for i in (1..len).rev() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let j = (seed as usize) % (i + 1);
            self.shuffle_order.swap(i, j);
        }
    }

    pub fn enqueue_track(&mut self, track: Track) {
        self.user_queue.push(track);
    }

    pub fn dequeue_track(&mut self, track_id: &str) {
        self.user_queue.retain(|t| t.id != track_id);
    }

    pub fn get_user_queue(&self) -> &[Track] {
        &self.user_queue
    }

    pub fn is_in_user_queue(&self, track_id: &str) -> bool {
        self.user_queue.iter().any(|t| t.id == track_id)
    }

    pub fn clear_user_queue(&mut self) {
        self.user_queue.clear();
    }

    pub fn set_continue_from_queue(&mut self, enabled: bool) {
        self.continue_from_queue = enabled;
    }

    pub fn continue_from_queue(&self) -> bool {
        self.continue_from_queue
    }

    /// Returns the queued track that needs a context update (not found in current context).
    pub fn pending_context_update(&self) -> Option<&Track> {
        self.pending_context_update.as_ref()
    }

    /// Replace context tracks and set current index to the given track, preserving user queue.
    pub fn update_context(&mut self, tracks: Vec<Track>, current_id: &str) {
        self.tracks = tracks;
        self.current_index = self.tracks.iter().position(|t| t.id == current_id);
        self.regenerate_shuffle();
        self.pending_context_update = None;
    }

    /// Returns (next_real_index, wrapped) where `wrapped` is true when
    /// the shuffle order has looped back to the start.
    fn next_shuffle_index(&self, current_real_index: usize) -> (usize, bool) {
        if let Some(pos) = self
            .shuffle_order
            .iter()
            .position(|&i| i == current_real_index)
        {
            let next_pos = (pos + 1) % self.shuffle_order.len();
            let wrapped = pos + 1 >= self.shuffle_order.len();
            (self.shuffle_order[next_pos], wrapped)
        } else {
            (0, false)
        }
    }
}
