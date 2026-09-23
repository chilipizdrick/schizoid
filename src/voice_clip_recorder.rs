use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;
use serenity::{all::prelude::TypeMapKey, async_trait, model::id::UserId};
use songbird::{Event, EventContext, EventHandler as VoiceEventHandler};

const BITS_PER_SAMPLE: u16 = 16;
const BLOCK_ALIGN: u16 = NUM_CHANNELS * (BITS_PER_SAMPLE / 8);
const BUFFER_SECONDS: usize = 60;
const BYTE_RATE: u32 = SAMPLE_RATE * (NUM_CHANNELS as u32) * (BITS_PER_SAMPLE as u32 / 8);
const MAX_SAMPLES: usize = (SAMPLE_RATE as usize) * BUFFER_SECONDS; // 2,880,000 samples (~5.76 MB)
const NUM_CHANNELS: u16 = 1;
const SAMPLE_RATE: u32 = 48_000;

#[derive(Default)]
pub struct VoiceClipRecorderState {
    pub ssrc_map: RwLock<HashMap<u32, UserId>>,
    pub buffers: RwLock<HashMap<UserId, UserAudioBuffer>>,
}

#[derive(Clone)]
pub struct VoiceClipRecorder {
    state: Arc<VoiceClipRecorderState>,
}

pub struct VCRStateKey;

impl TypeMapKey for VCRStateKey {
    type Value = Arc<VoiceClipRecorderState>;
}

impl VoiceClipRecorder {
    pub fn with_state(state: Arc<VoiceClipRecorderState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl VoiceEventHandler for VoiceClipRecorder {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let now = Instant::now();

        match ctx {
            EventContext::VoiceTick(tick) => {
                let ssrc_map = self.state.ssrc_map.read().clone();
                let mut buffers = self.state.buffers.write();

                for (ssrc, voice_data) in &tick.speaking {
                    if let Some(uid) = ssrc_map.get(ssrc) {
                        if let Some(decoded) = &voice_data.decoded_voice {
                            let buf = buffers.entry(*uid).or_default();
                            buf.push_frame(decoded, now);
                        }
                    }
                }
            }

            EventContext::SpeakingStateUpdate(speaking) => {
                if let Some(uid) = speaking.user_id {
                    let uid = UserId::from(uid.0);
                    self.state.ssrc_map.write().insert(speaking.ssrc, uid);
                    self.state.buffers.write().entry(uid).or_default();
                }
            }

            EventContext::ClientDisconnect(disconnect) => {
                let uid = UserId::from(disconnect.user_id.0);
                self.state.buffers.write().remove(&uid);
            }

            EventContext::DriverDisconnect(_) => {
                self.state.ssrc_map.write().clear();
                self.state.buffers.write().clear();
            }

            _ => {}
        }

        None
    }
}

pub struct UserAudioBuffer {
    buffer: Vec<i16>,
    write_idx: usize,
    total_written: usize,
    last_seen: Option<Instant>,
}

impl Default for UserAudioBuffer {
    fn default() -> Self {
        Self {
            buffer: vec![0i16; MAX_SAMPLES],
            write_idx: 0,
            total_written: 0,
            last_seen: None,
        }
    }
}

impl UserAudioBuffer {
    pub fn new() -> Self {
        Default::default()
    }

    /// Pushes incoming PCM samples, bulk-filling silence if the user was quiet.
    pub fn push_frame(&mut self, pcm: &[i16], now: Instant) {
        if let Some(last) = self.last_seen {
            let elapsed_ms = now.duration_since(last).as_millis() as usize;

            // Voice tick is ~20ms. If elapsed > 35ms and user was not inactive for > 60s:
            if elapsed_ms > 35 && elapsed_ms < (BUFFER_SECONDS * 1000) {
                let missing_samples = ((elapsed_ms - 20) * (SAMPLE_RATE as usize)) / 1000;
                let silence_to_fill = missing_samples.min(MAX_SAMPLES);
                self.write_silence(silence_to_fill);
            } else if elapsed_ms >= (BUFFER_SECONDS * 1000) {
                // If inactive for > 60 seconds, past history is expired
                self.write_idx = 0;
                self.total_written = 0;
            }
        }

        self.write_slice(pcm);
        self.last_seen = Some(now);
    }

    /// Returns [`None`] if underlying buffer is empty
    #[cfg(target_endian = "little")]
    pub fn to_wav_bytes(&self) -> Option<Vec<u8>> {
        if self.total_written == 0 {
            return None;
        }

        let data_len = (self.total_written * std::mem::size_of::<i16>()) as u32;
        let riff_chunk_size = 36 + data_len;

        let mut out = Vec::with_capacity(44 + data_len as usize);

        // Canonical 44-byte RIFF Header (Little Endian)
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&riff_chunk_size.to_le_bytes());
        out.extend_from_slice(b"WAVE");

        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&NUM_CHANNELS.to_le_bytes());
        out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        out.extend_from_slice(&BYTE_RATE.to_le_bytes());
        out.extend_from_slice(&BLOCK_ALIGN.to_le_bytes());
        out.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());

        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());

        let mut copy_chunk = |slice: &[i16]| {
            let byte_len = slice.len() * std::mem::size_of::<i16>();
            let byte_ptr = slice.as_ptr() as *const u8;
            let cur_len = out.len();
            out.reserve(byte_len);
            unsafe {
                std::ptr::copy_nonoverlapping(byte_ptr, out.as_mut_ptr().add(cur_len), byte_len);
                out.set_len(cur_len + byte_len);
            }
        };

        if self.total_written < MAX_SAMPLES {
            copy_chunk(&self.buffer[..self.write_idx]);
        } else {
            copy_chunk(&self.buffer[self.write_idx..]);
            copy_chunk(&self.buffer[..self.write_idx]);
        }

        Some(out)
    }

    /// Bulk fills `count` zeroes into the ring buffer via vectorized slice::fill.
    fn write_silence(&mut self, mut count: usize) {
        count = count.min(MAX_SAMPLES);
        let space_to_end = MAX_SAMPLES - self.write_idx;

        if count <= space_to_end {
            self.buffer[self.write_idx..self.write_idx + count].fill(0);
            self.write_idx = (self.write_idx + count) % MAX_SAMPLES;
        } else {
            self.buffer[self.write_idx..].fill(0);
            let remaining = count - space_to_end;
            self.buffer[..remaining].fill(0);
            self.write_idx = remaining;
        }

        self.total_written = (self.total_written + count).min(MAX_SAMPLES);
    }

    /// Bulk copies PCM slices into the ring buffer.
    fn write_slice(&mut self, mut data: &[i16]) {
        if data.is_empty() {
            return;
        }

        if data.len() > MAX_SAMPLES {
            data = &data[data.len() - MAX_SAMPLES..];
        }

        let count = data.len();
        let space_to_end = MAX_SAMPLES - self.write_idx;

        if count <= space_to_end {
            self.buffer[self.write_idx..self.write_idx + count].copy_from_slice(data);
            self.write_idx = (self.write_idx + count) % MAX_SAMPLES;
        } else {
            self.buffer[self.write_idx..].copy_from_slice(&data[..space_to_end]);
            let remaining = count - space_to_end;
            self.buffer[..remaining].copy_from_slice(&data[space_to_end..]);
            self.write_idx = remaining;
        }

        self.total_written = (self.total_written + count).min(MAX_SAMPLES);
    }
}
