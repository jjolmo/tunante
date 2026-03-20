use ogg::reading::PacketReader;
use opus_decoder::OpusDecoder;
use rodio::Source;
use std::io::{Read, Seek};
use std::num::NonZeroU16;
use std::num::NonZeroU32;
use std::time::Duration;

/// Streaming Ogg Opus decoder that implements rodio::Source.
/// Decodes packets on-demand as rodio pulls samples, avoiding upfront memory allocation.
pub struct OggOpusSource<R: Read + Seek> {
    packet_reader: PacketReader<R>,
    decoder: OpusDecoder,
    channels: NonZeroU16,
    channel_count: usize,
    // Current decoded frame buffer
    buffer: Vec<f32>,
    buf_pos: usize,
    // Pre-skip handling
    skip_remaining: usize,
    // Duration tracking
    total_duration: Option<Duration>,
    finished: bool,
}

impl<R: Read + Seek> OggOpusSource<R> {
    pub fn new(reader: R) -> Result<Self, String> {
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
        let pre_skip = u16::from_le_bytes([head[10], head[11]]) as usize;
        let input_sample_rate = u32::from_le_bytes([head[12], head[13], head[14], head[15]]);

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

        let decoder = OpusDecoder::new(48000, channel_count)
            .map_err(|e| format!("Failed to create Opus decoder: {:?}", e))?;

        let channels =
            NonZeroU16::new(channel_count as u16).ok_or("Invalid channel count")?;

        // Try to estimate duration from the last page's granule position.
        // We'd need to seek to the end, which is expensive. For now, use None
        // and let the timer handle display. If input_sample_rate is set in the
        // header, we at least know it's a valid file.
        let _ = input_sample_rate;

        Ok(Self {
            packet_reader,
            decoder,
            channels,
            channel_count,
            buffer: Vec::new(),
            buf_pos: 0,
            skip_remaining: pre_skip * channel_count,
            total_duration: None,
            finished: false,
        })
    }

    /// Decode the next Ogg packet into the internal buffer.
    /// Returns true if a packet was decoded, false if stream ended.
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

                            // Handle pre-skip
                            if self.skip_remaining > 0 {
                                let skip = self.skip_remaining.min(self.buffer.len());
                                self.buf_pos = skip;
                                self.skip_remaining -= skip;
                                // If we consumed the entire buffer with skip, decode another
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

        // If current buffer is exhausted, decode next packet
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
            // Return remaining samples in current buffer
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
}
