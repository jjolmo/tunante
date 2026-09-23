use tunante_core::vgm_path::build_vgm_path;
use tunante_core::db::models::Track;
use crate::metadata::vgmstream_reader::parse_folder_m3u_ratings;
use game_music_emu::GameMusicEmu;
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

/// Default play duration for tracks with unknown length in fast scan mode (2.5 minutes)
/// Fallback play time for tracks whose real length can't be determined —
/// in practice, the ones that loop forever. Overridable in Settings; this is
/// only the value used when no preference is stored.
pub const DEFAULT_DURATION_MS: i64 = 150_000;
/// Fade duration appended after play_length
const FADE_MS: i64 = 10_000;
/// Maximum time to emulate when detecting duration by silence (5 minutes)
const MAX_DETECT_DURATION_MS: i32 = 300_000;
/// Chunk size for silence detection (stereo samples per iteration)
const DETECT_CHUNK_SAMPLES: usize = 2048;

/// Parsed entry from a GME-style .m3u playlist.
pub(crate) struct M3uEntry {
    /// The song number exactly as the line names it — decimal, or the value of
    /// a `$99`. For a KSS rip this is the driver's song id and not a position,
    /// which is why nothing here indexes on it.
    pub(crate) track: i32,
    pub(crate) title: String,
    pub(crate) length_ms: i64, // -1 if not specified
    pub(crate) fade_ms: i64,   // -1 if not specified
}

/// Parsed data from a GME-style .m3u file: its entries, in file order.
///
/// Order is the payload here, not a convenience. `GameMusicEmu::from_file`
/// hands the same file to GME (`gme_load_m3u_data`), and when GME accepts it
/// the emulator's track list *becomes* these entries, in this order — so entry
/// *n* describes GME's track *n* whatever song number the line spells.
pub(crate) struct M3uData {
    pub(crate) entries: Vec<M3uEntry>,
}

/// Read an `.m3u` as text, whatever it was written in.
///
/// These are decades-old rips and hardly any of them are UTF-8: the titles in
/// a KSS or SPC playlist carry Japanese in Shift-JIS, and a European one is
/// usually Windows-1252. `read_to_string` refuses every one of them, and it
/// refuses the *whole file* over a single byte — which is how a Metal Gear rip
/// came to show 47 tracks named "Track 1" while its playlist sat next to it
/// with every real title in it.
fn read_m3u_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    match String::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(e) => {
            let bytes = e.into_bytes();
            // Shift-JIS first, and only when it fits: it is the encoding that
            // carries real meaning for these files, and `had_errors` says when
            // it is the wrong guess. Windows-1252 maps every possible byte, so
            // it can never report a problem — asking it first would silently
            // turn every Japanese title into mojibake.
            let (text, _, had_errors) = encoding_rs::SHIFT_JIS.decode(&bytes);
            if !had_errors {
                return Some(text.into_owned());
            }
            let (text, _, _) = encoding_rs::WINDOWS_1252.decode(&bytes);
            Some(text.into_owned())
        }
    }
}

/// The song number a track field names: `71`, or `$99` written in hex.
///
/// KSS rips number their songs the way the sound driver does — hex, with a
/// `$`. Most other formats use a plain 1-based index. Both turn up in the
/// wild, sometimes in the same folder.
fn parse_track_number(field: &str) -> Option<i32> {
    let s = field.trim();
    match s.strip_prefix('$') {
        Some(hex) => i32::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

/// Parse a GME-style .m3u file, keeping its entries in file order.
/// Format: `filename::TYPE,track,title,length,,fade`
/// or:     `filename,track,title,length,,fade`
pub(crate) fn parse_gme_m3u(path: &Path) -> Option<M3uData> {
    let content = read_m3u_text(path)?;
    let mut entries = Vec::new();

    for line in content.lines() {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Skip the filename part: either `name::TYPE,rest` or `name,rest`
        let rest = if let Some(pos) = line.find("::") {
            // `filename::TYPE,rest` — skip past the type and comma
            let after_type = &line[pos + 2..];
            after_type.find(',').map(|p| &after_type[p + 1..])
        } else {
            // `filename,rest` — skip past first comma, but only if followed by a digit
            line.find(',').and_then(|p| {
                let after = line[p + 1..].trim_start();
                if after.starts_with(|c: char| c.is_ascii_digit() || c == '$') {
                    Some(after)
                } else {
                    None
                }
            })
        };

        let rest = match rest {
            Some(r) => r,
            None => continue,
        };

        // Parse: track,title,length,,fade
        let fields: Vec<&str> = split_m3u_fields(rest);
        if fields.is_empty() {
            continue;
        }

        let track = match parse_track_number(fields[0]) {
            Some(t) => t,
            None => continue,
        };

        let title = if fields.len() > 1 {
            // Unescape \, sequences
            fields[1].replace("\\,", ",").trim().to_string()
        } else {
            String::new()
        };

        let length_ms = if fields.len() > 2 {
            parse_m3u_time(fields[2])
        } else {
            -1
        };

        // fields[3] is usually empty (loop intro), skip it
        let fade_ms = if fields.len() > 4 {
            parse_m3u_time(fields[4])
        } else {
            -1
        };

        entries.push(M3uEntry {
            track,
            title,
            length_ms,
            fade_ms,
        });
    }

    if entries.is_empty() {
        None
    } else {
        Some(M3uData { entries })
    }
}

/// Whether a playlist's entries line up with GME's tracks by *order* rather
/// than by the number each line names.
///
/// An ordinary NSF or GBS playlist numbers its lines 1..N and the number is an
/// index into the file's songs, written in whatever order the ripper felt
/// like: Batman's starts 10, 11, 1, 2…, so only the number puts its eleven
/// titles on the right eleven songs.
///
/// A KSS playlist numbers them the way the sound driver does — `$99`, `71`,
/// `$6A` — ids rather than positions, every one past the end of a ten-song
/// file. What aligns there is the order itself.
///
/// Both halves of the evidence are needed, because a number out of range has a
/// second explanation: a playlist that does not describe this file well (Mega
/// Man 4 ships 27 lines for a file GME reads as one song). So the order is
/// only trusted when GME's track list *is* this playlist — same count, because
/// `GameMusicEmu::from_file` handed GME the same file and a playlist it loads
/// becomes the track list. Otherwise the number is all there is, and a number
/// beats a guess.
fn entries_align_by_order(entries: &[M3uEntry], track_count: usize, is_kss: bool) -> bool {
    // KSS never numbers by position: the field is the sound driver's song id,
    // even on the rips whose ids happen to be small enough to look like
    // indices. Playback depends on this agreeing with `kss::song_number_for`,
    // which reads entry N for track N — so the answer here is not a guess.
    if is_kss {
        return true;
    }
    let numbers_are_indices = entries
        .iter()
        .all(|e| e.track >= 1 && (e.track as usize) <= track_count);
    let gme_loaded_this_playlist = entries.len() == track_count;
    !numbers_are_indices && gme_loaded_this_playlist
}

/// Split m3u fields by comma, respecting `\,` escapes.
fn split_m3u_fields(s: &str) -> Vec<&str> {
    let mut fields = Vec::new();
    let mut start = 0;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b',' && (i == 0 || bytes[i - 1] != b'\\') {
            fields.push(&s[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    fields.push(&s[start..]);
    fields
}

/// Parse GME m3u time format: `M:SS`, `M:SS.mmm`, or just seconds `SS`.
fn parse_m3u_time(s: &str) -> i64 {
    let s = s.trim();
    if s.is_empty() || s == "-" {
        return -1;
    }

    let (main_part, frac_ms) = if let Some(dot_pos) = s.find('.') {
        let frac_str = &s[dot_pos + 1..];
        let frac = match frac_str.len() {
            0 => 0i64,
            1 => frac_str.parse::<i64>().unwrap_or(0) * 100,
            2 => frac_str.parse::<i64>().unwrap_or(0) * 10,
            3 => frac_str.parse::<i64>().unwrap_or(0),
            _ => frac_str[..3].parse::<i64>().unwrap_or(0),
        };
        (&s[..dot_pos], frac)
    } else {
        (s, 0i64)
    };

    let parts: Vec<&str> = main_part.split(':').collect();
    let seconds: i64 = match parts.len() {
        1 => parts[0].parse::<i64>().unwrap_or(0),
        2 => {
            let min = parts[0].parse::<i64>().unwrap_or(0);
            let sec = parts[1].parse::<i64>().unwrap_or(0);
            min * 60 + sec
        }
        3 => {
            let hr = parts[0].parse::<i64>().unwrap_or(0);
            let min = parts[1].parse::<i64>().unwrap_or(0);
            let sec = parts[2].parse::<i64>().unwrap_or(0);
            hr * 3600 + min * 60 + sec
        }
        _ => 0,
    };

    seconds * 1000 + frac_ms
}

/// Detect the actual play duration of a GME track by emulating until silence.
/// Returns the duration in milliseconds, or None if the track loops past the max limit.
fn detect_duration_by_silence(path: &Path, track_index: usize) -> Option<i64> {
    let emu = GameMusicEmu::from_file(path, 44100).ok()?;
    emu.start_track(track_index).ok()?;
    emu.set_fade(MAX_DETECT_DURATION_MS);

    let mut buf = vec![0i16; DETECT_CHUNK_SAMPLES];
    loop {
        if emu.track_ended() {
            let ms = emu.tell() as i64;
            return Some(ms);
        }
        if emu.tell() as i32 >= MAX_DETECT_DURATION_MS {
            return None;
        }
        if emu.play(DETECT_CHUNK_SAMPLES, &mut buf).is_err() {
            return None;
        }
    }
}

/// Read all sub-tracks from a GME file, returning one Track per sub-track.
/// Parses matching .m3u playlist in Rust for track names, durations, and ordering.
///
/// When `fast_scan` is false, tracks without a known duration are emulated to detect
/// their actual length via silence detection. This is slower but gives accurate durations.
pub fn read_gme_metadata(path: &Path) -> Result<Vec<Track>, String> {
    read_gme_metadata_inner(path, false, DEFAULT_DURATION_MS, false)
}

/// Same as read_gme_metadata but allows controlling fast_scan mode.
pub fn read_gme_metadata_with_opts(
    path: &Path,
    fast_scan: bool,
    loop_max_ms: i64,
    cap_all: bool,
) -> Result<Vec<Track>, String> {
    read_gme_metadata_inner(path, fast_scan, loop_max_ms, cap_all)
}

fn read_gme_metadata_inner(
    path: &Path,
    fast_scan: bool,
    loop_max_ms: i64,
    cap_all: bool,
) -> Result<Vec<Track>, String> {
    let emu = GameMusicEmu::from_file(path, 44100)
        .map_err(|e| format!("GME error: {}", e))?;

    let track_count = emu.track_count();
    let file_meta = std::fs::metadata(path)
        .map_err(|e| format!("IO error: {}", e))?;

    let file_path_str = path.to_string_lossy().to_string();
    let file_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let codec = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_uppercase())
        .unwrap_or_default();

    let modified_at = file_meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let file_size = file_meta.len() as i64;

    // Try to load matching .m3u playlist (parsed in Rust — GME's C m3u loader is broken)
    let m3u_path = path.with_extension("m3u");
    let m3u_entries = parse_gme_m3u(&m3u_path);

    // Read ratings from folder-level _ratings.m3u
    let full_file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let m3u_ratings = if let Some(folder) = path.parent() {
        let folder_m3u = folder.join("_ratings.m3u");
        if folder_m3u.exists() {
            parse_folder_m3u_ratings(&folder_m3u, full_file_name)
        } else {
            HashMap::new()
        }
    } else {
        HashMap::new()
    };

    // Order or number? See `entries_align_by_order`.
    let match_by_position = m3u_entries
        .as_ref()
        .is_some_and(|m| entries_align_by_order(&m.entries, track_count, codec == "KSS"));

    // For KSS the playlist *is* the track list, and this reader stops asking
    // GME how many songs the file holds.
    //
    // Correctness, not tidiness. Playback no longer goes through GME for this
    // format (see `crate::kss`) and resolves `file.kss#N` by taking entry N of
    // this same playlist — so if the track list came from GME's own m3u parser
    // the two would have to agree line for line, and they do not: measured over
    // eleven real rips, GME drops one line in Knightmare 2 and one in Snatcher,
    // which slides every title and every song after it by one. One parser, no
    // drift.
    let kss_from_playlist = codec == "KSS"
        && m3u_entries.as_ref().is_some_and(|m| !m.entries.is_empty());

    // If m3u defines track order, use it; otherwise iterate 0..track_count
    let track_indices: Vec<usize> = match m3u_entries {
        Some(ref m3u) if kss_from_playlist => (0..m3u.entries.len()).collect(),
        // GME's list is already the playlist, in the playlist's order.
        Some(_) if match_by_position => (0..track_count).collect(),
        Some(ref m3u) => {
            // Use m3u track order (1-based → 0-based index for GME)
            let mut numbers: Vec<i32> = m3u.entries.iter().map(|e| e.track).collect();
            numbers.sort();
            numbers.iter().map(|&t| (t - 1).max(0) as usize).collect()
        }
        None => (0..track_count).collect(),
    };

    let mut tracks = Vec::with_capacity(track_indices.len());

    for (seq, &i) in track_indices.iter().enumerate() {
        if i >= track_count && !kss_from_playlist {
            continue;
        }

        // With a KSS playlist, `i` is a position and can run past what GME
        // thinks the file holds. GME is only asked for the header fields —
        // game, author — which are the same whichever song is named, so any
        // valid index answers them.
        let info_index = if kss_from_playlist {
            i.min(track_count.saturating_sub(1))
        } else {
            i
        };
        let info = match emu.track_info(info_index) {
            Ok(info) => info,
            Err(e) => {
                log::warn!("GME track info error for track {}: {}", i, e);
                continue;
            }
        };

        // The entry describing this track, matched the way the order was.
        let m3u_entry = m3u_entries.as_ref().and_then(|m| {
            if match_by_position {
                m.entries.get(seq)
            } else {
                m.entries.iter().find(|e| e.track == (i + 1) as i32)
            }
        });

        // Title: prefer m3u title, then GME song name, then filename
        let title = if let Some(entry) = m3u_entry {
            if !entry.title.is_empty() {
                // Strip "Game - Author - " prefix that Zophar's m3u often has
                let t = &entry.title;
                // Find last " - " and use everything after it as the actual title
                // But only if there are at least 2 " - " separators (game - author - title)
                let dashes: Vec<usize> = t.match_indices(" - ").map(|(pos, _)| pos).collect();
                if dashes.len() >= 2 {
                    t[dashes[dashes.len() - 1] + 3..].to_string()
                } else {
                    entry.title.clone()
                }
            } else if !info.song.is_empty() {
                info.song.clone()
            } else {
                format!("Track {}", seq + 1)
            }
        } else if !info.song.is_empty() {
            info.song.clone()
        } else if track_count == 1 {
            // One song: the file name is the only name there is.
            file_name.clone()
        } else {
            format!("Track {}", i + 1)
        };

        // A track that does not loop ends when its data does, and GME stops
        // there — so a fade tacked on to the reported length is time the file
        // never plays. That is how a 17.8 s VGM came to claim 27.8 s.
        //
        // Only an explicit 0 counts. GME returns -1 for "I do not know", which
        // is what an SPC reports while looping forever, and treating that as
        // "no loop" would strip the fade from most of the SNES library.
        let fade_for_this = if info.loop_length != 0 { FADE_MS } else { 0 };

        // Duration: prefer m3u length, then GME play_length, then silence detection, then default
        let duration_ms = if let Some(entry) = m3u_entry {
            if entry.length_ms > 0 {
                // An .m3u fade is an explicit instruction from whoever made the
                // rip, so it counts even when GME sees no loop.
                let fade = if entry.fade_ms > 0 { entry.fade_ms } else { fade_for_this };
                entry.length_ms + fade
            } else if info.play_length > 0 {
                info.play_length as i64 + fade_for_this
            } else if !fast_scan {
                detect_duration_by_silence(path, i).unwrap_or(loop_max_ms + FADE_MS)
            } else {
                loop_max_ms + FADE_MS
            }
        } else if info.play_length > 0 {
            info.play_length as i64 + fade_for_this
        } else if !fast_scan {
            detect_duration_by_silence(path, i).unwrap_or(loop_max_ms + FADE_MS)
        } else {
            loop_max_ms + FADE_MS
        };

        // Optional hard cap over the whole cascade.
        //
        // Applied as a ceiling, never as a replacement: a 30 s jingle with its
        // length written in the `.m3u` stays 30 s, while a 4-minute one gets
        // trimmed to the limit. Replacing would stretch short tracks to the
        // limit, which is not what a maximum means.
        let duration_ms = if cap_all {
            duration_ms.min(loop_max_ms + FADE_MS)
        } else {
            duration_ms
        };

        // Virtual path: only add #N for multi-track files
        let virtual_path = if track_count == 1 {
            file_path_str.clone()
        } else {
            build_vgm_path(&file_path_str, i)
        };

        // Map GME game → album (so tracks from the same game group together)
        let game = info.game.clone();
        let album = if game.is_empty() {
            // Fallback: use parent folder name as album
            path.parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| file_name.clone())
        } else {
            game.clone()
        };

        tracks.push(Track {
            id: Uuid::new_v4().to_string(),
            path: virtual_path,
            title,
            artist: info.author.clone(),
            album,
            header_game: game.clone(),
            // Deliberately blank, not `info.system`.
            //
            // GME's `system` is "Nintendo NES", "Super Nintendo" — a function of
            // the file format, so it says nothing the extension does not already
            // say, and `tunante_core::console` is now the one place that answers
            // that question. Putting it here was a fourth console table hiding
            // in a column meant for something else.
            //
            // It was also a plain bug: `games::index` prefers `album_artist` as
            // a game's attribution, so every GME rip in the library was credited
            // to the machine it ran on instead of to its composer. Existing rows
            // heal on the next scan, because `insert_track`'s upsert overwrites
            // this column.
            album_artist: String::new(),
            track_number: Some((seq + 1) as i32),
            disc_number: None,
            duration_ms,
            sample_rate: Some(44100),
            channels: Some(2),
            bitrate: None,
            codec: codec.clone(),
            file_size,
            has_artwork: false,
            rating: m3u_ratings.get(&((i as i32) + 1)).copied().unwrap_or(0),
            modified_at,
            ..Default::default()
        });
    }

    Ok(tracks)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    /// A playlist of raw bytes on disk, parsed. Bytes rather than `&str`
    /// because half of what this parser meets in the wild is not UTF-8.
    fn parse_bytes(tag: &str, body: &[u8]) -> Option<super::M3uData> {
        let dir = std::env::temp_dir().join(format!("m3u-{tag}"));
        std::fs::create_dir_all(&dir).ok()?;
        let f = dir.join("t.m3u");
        let mut fh = std::fs::File::create(&f).ok()?;
        fh.write_all(body).ok()?;
        drop(fh);
        parse_gme_m3u(&f)
    }

    fn parse_line(line: &str) -> Option<(i32, String)> {
        let d = parse_bytes(&format!("line-{}", line.len()), format!("{line}\n").as_bytes())?;
        let e = d.entries.first()?;
        Some((e.track, e.title.clone()))
    }

    /// What Tunante writes has to come back out of the parser Tunante reads
    /// with. The first version did not: it left the type field empty, and the
    /// title landed a column over.
    #[test]
    fn a_written_line_round_trips_through_the_parser() {
        assert_eq!(parse_line("Metroid.nsf::NSF,1,Intro,"), Some((1, "Intro".into())));
    }

    /// The shape that was actually shipped, kept as a test so the bug is named
    /// rather than remembered.
    #[test]
    fn an_empty_type_field_does_not_yield_the_title() {
        let got = parse_line("Metroid.nsf::,1,,Intro");
        assert_ne!(got, Some((1, "Intro".into())), "this is what went wrong");
    }

    /// KSS rips number their songs in hex, the way the sound driver does.
    /// `"$99".parse::<i32>()` fails, and the line used to be dropped with it —
    /// so every Metal Gear 2 track came out named "Track N".
    #[test]
    fn a_hex_song_number_is_a_song_number() {
        assert_eq!(
            parse_line("mg2.kss::KSS,$99,THEME OF SOLID SNAKE,3:18,,5"),
            Some((0x99, "THEME OF SOLID SNAKE".into()))
        );
    }

    /// Decimal keeps working, and keeps meaning decimal: `$21` is 33, but a
    /// bare `21` must stay 21.
    #[test]
    fn a_decimal_song_number_is_not_read_as_hex() {
        assert_eq!(parse_line("mg.kss::KSS,21,TITLE,2,,0"), Some((21, "TITLE".into())));
    }

    /// One byte of Shift-JIS used to cost the whole file: `read_to_string`
    /// refuses it, and the parser dropped every entry including the ASCII ones.
    #[test]
    fn a_shift_jis_playlist_is_read_rather_than_dropped() {
        // "mg.kss::KSS,41,OPERATION INTRUDE N313 (オープニングBGM),10,,0"
        let mut body = b"mg.kss::KSS,41,OPERATION INTRUDE N313 (".to_vec();
        body.extend_from_slice(&[
            0x83, 0x49, 0x81, 0x5b, 0x83, 0x76, 0x83, 0x6a, 0x83, 0x93, 0x83, 0x4f,
        ]);
        body.extend_from_slice(b"BGM),10,,0\n");

        let d = parse_bytes("sjis", &body).expect("playlist survives non-UTF-8 bytes");
        let e = d.entries.first().expect("its one entry");
        assert_eq!(e.track, 41);
        assert!(
            e.title.starts_with("OPERATION INTRUDE N313"),
            "got {:?}",
            e.title
        );
        assert!(
            e.title.contains("オープニング"),
            "Shift-JIS decoded, not mojibake: {:?}",
            e.title
        );
    }

    /// Order is what pairs an entry with a GME track, so the parser has to
    /// keep it — a HashMap keyed on the song number did not, and for KSS the
    /// numbers are not positions at all.
    #[test]
    fn entries_keep_the_order_they_were_written_in() {
        let body = b"g.kss::KSS,$99,first,1,,0\ng.kss::KSS,$41,second,1,,0\ng.kss::KSS,$9A,third,1,,0\n";
        let d = parse_bytes("order", body).expect("parsed");
        let titles: Vec<&str> = d.entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["first", "second", "third"]);
    }

    fn numbered(tracks: &[i32]) -> Vec<super::M3uEntry> {
        tracks
            .iter()
            .map(|&track| super::M3uEntry {
                track,
                title: String::new(),
                length_ms: -1,
                fade_ms: -1,
            })
            .collect()
    }

    /// Batman's playlist: eleven lines numbered 1..11 and written 10, 11, 1,
    /// 2… Reading those in order shifts every title two songs along, which is
    /// exactly what it did until this was pinned down.
    #[test]
    fn indices_written_out_of_order_are_still_indices() {
        let e = numbered(&[10, 11, 1, 2, 3, 4, 6, 5, 7, 8, 9]);
        assert!(!entries_align_by_order(&e, 11, false));
    }

    /// Metal Gear on MSX: ten lines carrying the driver's song numbers, none of
    /// which is a position in a ten-song file. GME loaded the same playlist, so
    /// its track list is these entries in this order.
    #[test]
    fn song_ids_past_the_end_align_by_order() {
        let e = numbered(&[71, 41, 44, 47, 53, 62, 56, 59, 65, 68]);
        assert!(entries_align_by_order(&e, 10, false));
    }

    /// Solstice: numbered 1..19 for a ten-song file, so three lines fell off
    /// the end and the library held seven tracks of the ten GME reports — with
    /// the seven misnamed, "Title" playing under the name and length of
    /// "Begin".
    #[test]
    fn numbers_that_overshoot_align_by_order() {
        let e = numbered(&[6, 1, 2, 5, 4, 3, 7, 17, 18, 19]);
        assert!(entries_align_by_order(&e, 10, false));
    }

    /// Mega Man 4: 27 lines for a file GME reads as a single song. The numbers
    /// are out of range, but the playlist plainly does not describe this file,
    /// so its order is worth nothing and the number is all there is.
    #[test]
    fn a_playlist_that_is_not_gmes_track_list_is_matched_by_number() {
        let e = numbered(&[11, 68, 19, 14, 17, 16, 8, 7, 6, 4, 2, 1]);
        assert!(!entries_align_by_order(&e, 1, false));
    }

    use super::*;
}
