use ogg::reading::PacketReader;
use opus_decoder::OpusDecoder;
use rodio::Source;
use std::io::{Read, Seek};
use std::num::NonZeroU16;
use std::num::NonZeroU32;
use std::time::Duration;

/// Ogg Opus decoder that implements rodio::Source.
/// Decodes Opus packets from an Ogg container and outputs interleaved f32 PCM at 48kHz.
pub struct OggOpusSource {
    samples: Vec<f32>,
    pos: usize,
    channels: NonZeroU16,
    total_pcm_samples: Option<u64>,
}

impl OggOpusSource {
    pub fn new<R: Read + Seek>(reader: R) -> Result<Self, String> {
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

        // Decode all audio packets
        let mut decoder = OpusDecoder::new(48000, channel_count)
            .map_err(|e| format!("Failed to create Opus decoder: {:?}", e))?;

        let max_frame = decoder.max_frame_size_per_channel();
        let mut decode_buf = vec![0f32; max_frame * channel_count];
        let mut all_samples: Vec<f32> = Vec::new();
        let mut last_absgp = 0u64;

        loop {
            match packet_reader.read_packet() {
                Ok(Some(packet)) => {
                    last_absgp = packet.absgp_page();
                    match decoder.decode_float(&packet.data, &mut decode_buf, false) {
                        Ok(samples_per_channel) => {
                            let total = samples_per_channel * channel_count;
                            all_samples.extend_from_slice(&decode_buf[..total]);
                        }
                        Err(e) => {
                            log::warn!("Opus decode error (skipping frame): {:?}", e);
                        }
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    log::warn!("Ogg read error: {}", e);
                    break;
                }
            }
        }

        // Apply pre-skip: remove the first pre_skip samples (per channel)
        let skip_samples = pre_skip * channel_count;
        if skip_samples < all_samples.len() {
            all_samples.drain(..skip_samples);
        }

        // Trim end based on granule position if available
        let total_pcm_samples = if last_absgp > pre_skip as u64 {
            Some(last_absgp - pre_skip as u64)
        } else {
            None
        };

        if let Some(total) = total_pcm_samples {
            let total_interleaved = (total as usize) * channel_count;
            if total_interleaved < all_samples.len() {
                all_samples.truncate(total_interleaved);
            }
        }

        let channels = NonZeroU16::new(channel_count as u16)
            .ok_or("Invalid channel count")?;

        Ok(Self {
            samples: all_samples,
            pos: 0,
            channels,
            total_pcm_samples,
        })
    }
}

impl Iterator for OggOpusSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.pos < self.samples.len() {
            let sample = self.samples[self.pos];
            self.pos += 1;
            Some(sample)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.samples.len() - self.pos;
        (remaining, Some(remaining))
    }
}

impl Source for OggOpusSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.samples.len() - self.pos)
    }

    fn channels(&self) -> NonZeroU16 {
        self.channels
    }

    fn sample_rate(&self) -> NonZeroU32 {
        // SAFETY: 48000 is non-zero
        NonZeroU32::new(48000).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_pcm_samples.map(|samples| {
            Duration::from_secs_f64(samples as f64 / 48000.0)
        })
    }
}
