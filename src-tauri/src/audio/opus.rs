use ogg::reading::PacketReader;
use opus_decoder::OpusDecoder;
use rodio::source::SeekError;
use rodio::Source;
use std::io::{Read, Seek, SeekFrom};
use std::num::NonZeroU16;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

/// Streaming Ogg Opus decoder that implements rodio::Source.
/// Decodes packets on-demand as rodio pulls samples, avoiding upfront memory allocation.
pub struct OggOpusSource<R: Read + Seek> {
    packet_reader: PacketReader<R>,
    decoder: OpusDecoder,
    channels: NonZeroU16,
    channel_count: usize,
    buffer: Vec<f32>,
    buf_pos: usize,
    skip_remaining: usize,
    pre_skip: u64,
    total_duration: Option<Duration>,
    finished: bool,
}

/// Find the total duration by reading the last Ogg page's granule position.
/// Ogg page header: bytes 0-3 = "OggS", bytes 6-13 = granule position (u64 LE).
fn find_ogg_duration<R: Read + Seek>(reader: &mut R, pre_skip: u64) -> Option<Duration> {
    let file_size = reader.seek(SeekFrom::End(0)).ok()?;
    // Read the last 64KB (or whole file if smaller) to find the last "OggS" page
    let scan_size = 65536u64.min(file_size);
    let scan_start = file_size - scan_size;
    reader.seek(SeekFrom::Start(scan_start)).ok()?;

    let mut buf = vec![0u8; scan_size as usize];
    reader.read_exact(&mut buf).ok()?;

    // Scan backwards for the last "OggS" magic
    let mut last_granule = None;
    for i in (0..buf.len().saturating_sub(14)).rev() {
        if &buf[i..i + 4] == b"OggS" {
            let granule = u64::from_le_bytes([
                buf[i + 6],
                buf[i + 7],
                buf[i + 8],
                buf[i + 9],
                buf[i + 10],
                buf[i + 11],
                buf[i + 12],
                buf[i + 13],
            ]);
            // Granule position of -1 (0xFFFFFFFFFFFFFFFF) means "no position"
            if granule != u64::MAX {
                last_granule = Some(granule);
                break;
            }
        }
    }

    // Seek back to start for the PacketReader
    reader.seek(SeekFrom::Start(0)).ok()?;

    let granule = last_granule?;
    let pcm_samples = granule.saturating_sub(pre_skip);
    Some(Duration::from_secs_f64(pcm_samples as f64 / 48000.0))
}

impl<R: Read + Seek> OggOpusSource<R> {
    pub fn new(mut reader: R) -> Result<Self, String> {
        // First pass: quickly scan for duration from last Ogg page
        // We need to read headers first to get pre_skip, so do a two-step:
        // 1. Read OpusHead manually to get pre_skip
        // 2. Scan for duration
        // 3. Seek back and create PacketReader for streaming

        // Read first few bytes to get pre_skip from OpusHead
        let mut head_buf = [0u8; 19];
        reader
            .read_exact(&mut head_buf)
            .map_err(|e| format!("Failed to read OpusHead: {}", e))?;

        // Validate: OpusHead is inside an Ogg page, so we need to skip the Ogg page header.
        // Actually, let's just use PacketReader properly - seek back and use it.
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|e| format!("Seek failed: {}", e))?;

        let mut packet_reader = PacketReader::new(reader);

        // Packet 1: OpusHead
        let head_packet = packet_reader
            .read_packet_expected()
            .map_err(|e| format!("Failed to read OpusHead: {}", e))?;

        let head = &head_packet.data;
        if head.len() < 19 || &head[0..8] != b"OpusHead" {
            return Err("Invalid OpusHead packet".into());
        }

        let channel_count = head[9] as usize;
        let pre_skip = u16::from_le_bytes([head[10], head[11]]) as u64;

        if channel_count == 0 || channel_count > 2 {
            return Err(format!(
                "Unsupported channel count: {} (only mono/stereo supported)",
                channel_count
            ));
        }

        // Packet 2: OpusTags (skip)
        let _ = packet_reader
            .read_packet_expected()
            .map_err(|e| format!("Failed to read OpusTags: {}", e))?;

        // Now scan for duration using the underlying reader
        // We need to get the reader back, scan, then recreate PacketReader
        let mut reader = packet_reader.into_inner();
        let total_duration = find_ogg_duration(&mut reader, pre_skip);

        // Recreate PacketReader - it will re-read from start, so skip headers again
        let mut packet_reader = PacketReader::new(reader);
        let _ = packet_reader.read_packet_expected(); // OpusHead
        let _ = packet_reader.read_packet_expected(); // OpusTags

        let decoder = OpusDecoder::new(48000, channel_count)
            .map_err(|e| format!("Failed to create Opus decoder: {:?}", e))?;

        let channels =
            NonZeroU16::new(channel_count as u16).ok_or("Invalid channel count")?;

        Ok(Self {
            packet_reader,
            decoder,
            channels,
            channel_count,
            buffer: Vec::new(),
            buf_pos: 0,
            skip_remaining: pre_skip as usize * channel_count,
            pre_skip,
            total_duration,
            finished: false,
        })
    }

    fn decode_next_packet(&mut self) -> bool {
        loop {
            match self.packet_reader.read_packet() {
                Ok(Some(packet)) => {
                    let max_frame = self.decoder.max_frame_size_per_channel();
                    self.buffer.resize(max_frame * self.channel_count, 0.0);

                    match self.decoder.decode_float(&packet.data, &mut self.buffer, false) {
                        Ok(samples_per_channel) => {
                            let total = samples_per_channel * self.channel_count;
                            self.buffer.truncate(total);
                            self.buf_pos = 0;

                            if self.skip_remaining > 0 {
                                let skip = self.skip_remaining.min(self.buffer.len());
                                self.buf_pos = skip;
                                self.skip_remaining -= skip;
                                if self.buf_pos >= self.buffer.len() {
                                    continue;
                                }
                            }

                            return true;
                        }
                        Err(e) => {
                            log::warn!("Opus decode error (skipping frame): {:?}", e);
                            continue;
                        }
                    }
                }
                Ok(None) => {
                    self.finished = true;
                    return false;
                }
                Err(e) => {
                    log::warn!("Ogg read error: {}", e);
                    self.finished = true;
                    return false;
                }
            }
        }
    }
}

impl<R: Read + Seek + Send> Iterator for OggOpusSource<R> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.finished {
            return None;
        }

        if self.buf_pos >= self.buffer.len() {
            if !self.decode_next_packet() {
                return None;
            }
        }

        let sample = self.buffer[self.buf_pos];
        self.buf_pos += 1;
        Some(sample)
    }
}

impl<R: Read + Seek + Send> Source for OggOpusSource<R> {
    fn current_span_len(&self) -> Option<usize> {
        if self.finished {
            Some(0)
        } else {
            Some(self.buffer.len().saturating_sub(self.buf_pos))
        }
    }

    fn channels(&self) -> NonZeroU16 {
        self.channels
    }

    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(48000).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        // Convert target position to Ogg granule position (48kHz + pre_skip offset)
        let target_granule = (pos.as_secs_f64() * 48000.0) as u64 + self.pre_skip;

        self.packet_reader
            .seek_absgp(None, target_granule)
            .map_err(|e| SeekError::Other(Arc::new(e)))?;

        // Reset decoder state after seeking
        self.decoder.reset();
        self.buffer.clear();
        self.buf_pos = 0;
        self.skip_remaining = 0;
        self.finished = false;

        Ok(())
    }
}
