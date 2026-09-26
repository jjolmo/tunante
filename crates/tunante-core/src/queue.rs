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
    /// Bumped every time the context list is replaced, so a caller keeping a
    /// copy of it (the saved session) knows when that copy went stale.
    generation: u64,
    /// Where the list resumes when what is playing is not in it: a filter
    /// hid the current track, and the next one is the first still in the list
    /// that came after it. `next()` plays index `k`, `prev()` plays `k - 1`.
    /// Only meaningful while `current_index` is `None`.
    resume_at: Option<usize>,
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
            generation: 0,
            resume_at: None,
        }
    }

    pub fn set_tracks(&mut self, tracks: Vec<Track>) {
        self.tracks = tracks;
        self.generation += 1;
        self.resume_at = None;
        self.current_index = None;
        self.regenerate_shuffle();
    }

    pub fn play_index(&mut self, index: usize) -> Option<&Track> {
        if index < self.tracks.len() {
            self.current_index = Some(index);
            self.resume_at = None;
            Some(&self.tracks[index])
        } else {
            None
        }
    }

    pub fn play_track_by_id(&mut self, id: &str) -> Option<&Track> {
        if let Some(idx) = self.tracks.iter().position(|t| t.id == id) {
            self.current_index = Some(idx);
            self.resume_at = None;
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

    /// Changes whenever the context list does. See the field.
    pub fn generation(&self) -> u64 {
        self.generation
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
        let mut cursor = self.cursor();
        self.resume_at = None;
        // Try up to `len` candidates to avoid infinite loops when all tracks are short
        for _ in 0..len {
            let candidate_idx = match self.repeat {
                RepeatMode::One => {
                    return self.current_index.map(|i| self.tracks[i].clone());
                }
                RepeatMode::All => match cursor {
                    Some(i) => {
                        if self.shuffle {
                            self.next_shuffle_index(i).0
                        } else {
                            (i + 1) % len
                        }
                    }
                    None => 0,
                },
                RepeatMode::Off => match cursor {
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
            cursor = Some(candidate_idx);
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
        let mut current = self.cursor();
        for _ in 0..len {
            let candidate_idx = match self.repeat {
                RepeatMode::One => return self.current_index.map(|i| &self.tracks[i]),
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
        // Resuming at `k`, the one before is `k - 1`: step back from `k`.
        let mut current = self.current_index.or(self.resume_at);
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
        // Resuming at `k`, the one before is `k - 1`: step back from `k`.
        if let (None, Some(k)) = (self.current_index, self.resume_at) {
            if k == 0 && self.repeat != RepeatMode::All {
                return None;
            }
            self.resume_at = None;
            self.current_index = Some(k);
        }
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
        self.generation += 1;
        self.resume_at = None;
        self.current_index = self.tracks.iter().position(|t| t.id == current_id);
        self.regenerate_shuffle();
        self.pending_context_update = None;
    }

    /// Replace the list with one that does not hold the playing track: it
    /// keeps playing, and the list resumes at `next` — `next()` plays that
    /// index, `prev()` the one before it. `next == len` means nothing in the
    /// list came after it.
    pub fn update_context_resuming_at(&mut self, tracks: Vec<Track>, next: usize) {
        self.tracks = tracks;
        self.generation += 1;
        self.current_index = None;
        self.resume_at = Some(next.min(self.tracks.len()));
        self.regenerate_shuffle();
        self.pending_context_update = None;
    }

    /// Where the list resumes while the playing track is not in it. See the
    /// field.
    pub fn resume_at(&self) -> Option<usize> {
        if self.current_index.is_none() { self.resume_at } else { None }
    }

    /// Replace the list without losing the place in it: the current track
    /// stays current; if it is gone, the list resumes where it was; and a
    /// list that was already resuming resumes at the same track, or — that
    /// one gone too — at the same row.
    ///
    /// What adding to, removing from or reordering the list needs. Before,
    /// they only knew how to hold on to a current track, and with none (the
    /// filter had hidden it) they dropped the place and started over.
    pub fn replace_keeping_place(&mut self, tracks: Vec<Track>) {
        let (target, row) = match (self.current_index, self.resume_at) {
            (Some(i), _) => {
                let id = self.tracks[i].id.clone();
                if tracks.iter().any(|t| t.id == id) {
                    self.update_context(tracks, &id);
                    return;
                }
                // The playing track left the list: resume at the row it
                // held, which is now the one after it.
                (None, i)
            }
            (None, Some(k)) => (self.tracks.get(k).map(|t| t.id.clone()), k),
            (None, None) => {
                self.set_tracks(tracks);
                return;
            }
        };
        let next = target
            .and_then(|id| tracks.iter().position(|t| t.id == id))
            .unwrap_or_else(|| row.min(tracks.len()));
        self.update_context_resuming_at(tracks, next);
    }

    /// Where `next()` and `peek_next()` start counting from: the current
    /// track, or — when it is not in the list — the one just before where the
    /// list resumes, so that the step after it lands on `resume_at`.
    fn cursor(&self) -> Option<usize> {
        match (self.current_index, self.resume_at) {
            (Some(i), _) => Some(i),
            (None, Some(k)) => k.checked_sub(1),
            (None, None) => None,
        }
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

    /// The saved session rewrites the list only when it changed; moving
    /// through it must not count as a change, replacing it must.
    #[test]
    fn the_generation_moves_with_the_list_not_with_playback() {
        let mut q = PlayQueue::new();
        let g0 = q.generation();
        q.set_tracks(vec![t("a"), t("b")]);
        let g1 = q.generation();
        assert_ne!(g0, g1);
        q.play_index(0);
        q.next();
        q.prev();
        assert_eq!(q.generation(), g1);
        q.update_context(vec![t("b"), t("a")], "a");
        assert_ne!(q.generation(), g1);
        assert_eq!(q.current().map(|t| t.id.as_str()), Some("a"));
    }

    /// A list that no longer holds the playing track resumes where it says:
    /// next plays that row, previous the one before, and resuming past the
    /// end is the end of the list.
    #[test]
    fn resuming_at_a_row_steps_from_there() {
        let mut q = PlayQueue::new();
        q.update_context_resuming_at(vec![t("a"), t("b"), t("c")], 1);
        assert!(q.current().is_none());
        assert_eq!(q.peek_next().map(|t| t.id.as_str()), Some("b"));
        assert_eq!(q.peek_prev().map(|t| t.id.as_str()), Some("a"));
        assert_eq!(q.next().map(|t| t.id), Some("b".into()));
        assert_eq!(q.next().map(|t| t.id), Some("c".into()));

        q.update_context_resuming_at(vec![t("a"), t("b")], 1);
        assert_eq!(q.prev().map(|t| t.id), Some("a".into()));

        q.update_context_resuming_at(vec![t("a"), t("b")], 0);
        assert!(q.prev().is_none(), "nothing before the first row");
        assert!(q.current().is_none(), "and no row claimed while refusing");
        assert_eq!(q.next().map(|t| t.id), Some("a".into()));

        q.update_context_resuming_at(vec![t("a"), t("b")], 2);
        assert!(q.next().is_none(), "nothing shown came after it");
    }

    /// Adding to, removing from or reordering a list whose playing track a
    /// filter hid keeps the place instead of starting over.
    #[test]
    fn changing_a_resuming_list_keeps_the_place() {
        let mut q = PlayQueue::new();
        // Playing a hidden track; next is b (row 1).
        q.update_context_resuming_at(vec![t("a"), t("b"), t("c")], 1);

        q.replace_keeping_place(vec![t("a"), t("b"), t("c"), t("z")]);
        assert_eq!(q.peek_next().map(|t| t.id.as_str()), Some("b"), "adding");

        q.replace_keeping_place(vec![t("c"), t("a"), t("b"), t("z")]);
        assert_eq!(q.peek_next().map(|t| t.id.as_str()), Some("b"), "reordering follows the track");

        q.replace_keeping_place(vec![t("c"), t("a"), t("z")]);
        assert_eq!(q.peek_next().map(|t| t.id.as_str()), Some("z"), "removing it resumes at its row");

        // Resuming past the end, and more arrives: that is what comes next.
        q.update_context_resuming_at(vec![t("a")], 1);
        q.replace_keeping_place(vec![t("a"), t("n")]);
        assert_eq!(q.peek_next().map(|t| t.id.as_str()), Some("n"));
    }

    /// Removing the playing row itself: it plays on, the list resumes at the
    /// row that took its place.
    #[test]
    fn removing_the_playing_row_resumes_where_it_was() {
        let mut q = PlayQueue::new();
        q.set_tracks(vec![t("a"), t("b"), t("c")]);
        q.play_index(1);
        q.replace_keeping_place(vec![t("a"), t("c")]);
        assert!(q.current().is_none());
        assert_eq!(q.peek_next().map(|t| t.id.as_str()), Some("c"));
    }
}
