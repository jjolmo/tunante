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
    /// Minimum duration in ms — context tracks shorter than this are skipped.
    /// User-queued tracks are never filtered.
    short_filter_threshold_ms: i64,
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
            short_filter_threshold_ms: 0,
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

    /// The context list, in queue order.
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Where the current track sits in that list.
    ///
    /// Note this is the position in the *context*, not in shuffle order: it is
    /// what a UI needs to mark the playing row, which the user sees in list
    /// order regardless of how the next track gets chosen.
    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    /// Whether shuffle is on.
    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// The repeat mode in effect.
    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn current(&self) -> Option<&Track> {
        self.current_index.and_then(|i| self.tracks.get(i))
    }

    pub fn next(&mut self) -> Option<Track> {
        // User queue takes priority — never filtered
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

        let len = self.tracks.len();
        // Try up to `len` candidates to avoid infinite loops when all tracks are short
        for _ in 0..len {
            let candidate_idx = match self.repeat {
                RepeatMode::One => {
                    return self.current_index.map(|i| self.tracks[i].clone());
                }
                RepeatMode::All => match self.current_index {
                    Some(i) => {
                        if self.shuffle {
                            self.next_shuffle_index(i).0
                        } else {
                            (i + 1) % len
                        }
                    }
                    None => 0,
                },
                RepeatMode::Off => match self.current_index {
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
                },
            };

            if candidate_idx >= len {
                return None;
            }

            self.current_index = Some(candidate_idx);
            let track = &self.tracks[candidate_idx];
            if self.short_filter_threshold_ms <= 0
                || track.duration_ms >= self.short_filter_threshold_ms
            {
                return Some(track.clone());
            }
            // Track is too short, loop to try the next one
        }

        None
    }

    /// The track `next()` would return, without moving. What the Playing
    /// screen's carousel shows on the card sliding in from the right: the
    /// head of the hand-built queue if there is one, else the context's next
    /// under the current repeat/shuffle rules, skipping the short filter the
    /// same way `next()` does. `None` where `next()` would stop.
    pub fn peek_next(&self) -> Option<&Track> {
        if let Some(t) = self.user_queue.first() {
            return Some(t);
        }
        if self.tracks.is_empty() {
            return None;
        }
        let len = self.tracks.len();
        let mut current = self.current_index;
        for _ in 0..len {
            let candidate_idx = match self.repeat {
                RepeatMode::One => return current.map(|i| &self.tracks[i]),
                RepeatMode::All => match current {
                    Some(i) => {
                        if self.shuffle {
                            self.next_shuffle_index(i).0
                        } else {
                            (i + 1) % len
                        }
                    }
                    None => 0,
                },
                RepeatMode::Off => match current {
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
                },
            };
            if candidate_idx >= len {
                return None;
            }
            let track = &self.tracks[candidate_idx];
            if self.short_filter_threshold_ms <= 0
                || track.duration_ms >= self.short_filter_threshold_ms
            {
                return Some(track);
            }
            current = Some(candidate_idx);
        }
        None
    }

    /// The track `prev()` would return, without moving. Same rules, same
    /// short-filter skipping. `None` where `prev()` would stop.
    pub fn peek_prev(&self) -> Option<&Track> {
        if self.tracks.is_empty() {
            return None;
        }
        let len = self.tracks.len();
        let mut current = self.current_index;
        for _ in 0..len {
            let prev_idx = match current {
                Some(i) if i > 0 => i - 1,
                Some(_) => {
                    if self.repeat == RepeatMode::All {
                        len - 1
                    } else {
                        return None;
                    }
                }
                None => 0,
            };
            let track = &self.tracks[prev_idx];
            if self.short_filter_threshold_ms <= 0
                || track.duration_ms >= self.short_filter_threshold_ms
            {
                return Some(track);
            }
            current = Some(prev_idx);
        }
        None
    }

    pub fn prev(&mut self) -> Option<Track> {
        if self.tracks.is_empty() {
            return None;
        }

        let len = self.tracks.len();
        for _ in 0..len {
            let prev_idx = match self.current_index {
                Some(i) if i > 0 => i - 1,
                Some(_) => {
                    if self.repeat == RepeatMode::All {
                        len - 1
                    } else {
                        return None;
                    }
                }
                None => 0,
            };

            self.current_index = Some(prev_idx);
            let track = &self.tracks[prev_idx];
            if self.short_filter_threshold_ms <= 0
                || track.duration_ms >= self.short_filter_threshold_ms
            {
                return Some(track.clone());
            }
        }

        None
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
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
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

    /// Move a waiting track from one position to another.
    ///
    /// Both indices are clamped rather than rejected: a drag that ends past the
    /// end of the list means "put it last", which is what the finger was saying.
    pub fn move_in_user_queue(&mut self, from: usize, to: usize) {
        if self.user_queue.is_empty() || from >= self.user_queue.len() {
            return;
        }
        let to = to.min(self.user_queue.len() - 1);
        if from == to {
            return;
        }
        let track = self.user_queue.remove(from);
        self.user_queue.insert(to, track);
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

    pub fn set_short_filter(&mut self, threshold_ms: i64) {
        self.short_filter_threshold_ms = threshold_ms;
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

#[cfg(test)]
mod user_queue_tests {
    use super::*;
    use crate::db::models::Track;

    fn t(id: &str) -> Track {
        Track {
            id: id.into(), path: format!("/m/{id}.psf"), title: id.into(),
            artist: String::new(), album: String::new(), album_artist: String::new(),
            track_number: None, disc_number: None, duration_ms: 1000,
            sample_rate: None, channels: None, bitrate: None,
            codec: "test".into(), file_size: 0, has_artwork: false, rating: 0,
            modified_at: 0,
            ..Default::default()
        }
    }

    fn queue_of(ids: &[&str]) -> PlayQueue {
        let mut q = PlayQueue::new();
        for id in ids { q.enqueue_track(t(id)); }
        q
    }

    fn ids(q: &PlayQueue) -> Vec<String> {
        q.get_user_queue().iter().map(|t| t.id.clone()).collect()
    }

    #[test]
    fn moving_forward_lands_where_the_finger_stopped() {
        let mut q = queue_of(&["a", "b", "c", "d"]);
        q.move_in_user_queue(0, 2);
        assert_eq!(ids(&q), ["b", "c", "a", "d"]);
    }

    #[test]
    fn moving_backward_does_too() {
        let mut q = queue_of(&["a", "b", "c", "d"]);
        q.move_in_user_queue(3, 1);
        assert_eq!(ids(&q), ["a", "d", "b", "c"]);
    }

    /// A drag that ends past the end means "last", not "nothing".
    #[test]
    fn dropping_past_the_end_puts_it_last() {
        let mut q = queue_of(&["a", "b", "c"]);
        q.move_in_user_queue(0, 99);
        assert_eq!(ids(&q), ["b", "c", "a"]);
    }

    #[test]
    fn moving_onto_itself_changes_nothing() {
        let mut q = queue_of(&["a", "b", "c"]);
        q.move_in_user_queue(1, 1);
        assert_eq!(ids(&q), ["a", "b", "c"]);
    }

    /// Out of range on the way in is a stale index from a list that already
    /// moved, and it must not panic or scramble the queue.
    #[test]
    fn a_stale_index_is_ignored() {
        let mut q = queue_of(&["a", "b"]);
        q.move_in_user_queue(9, 0);
        assert_eq!(ids(&q), ["a", "b"]);
        let mut empty = PlayQueue::new();
        empty.move_in_user_queue(0, 0);
        assert!(empty.get_user_queue().is_empty());
    }

    /// The queue is a layer over the context: taking from it must not disturb
    /// the folder underneath.
    #[test]
    fn reordering_leaves_the_context_alone() {
        let mut q = queue_of(&["a", "b"]);
        q.set_tracks(vec![t("x"), t("y")]);
        q.move_in_user_queue(0, 1);
        assert_eq!(q.tracks().len(), 2);
        assert_eq!(ids(&q), ["b", "a"]);
    }
}
