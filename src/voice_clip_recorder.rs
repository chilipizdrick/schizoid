use std::{collections::HashMap, io, sync::Arc};

use hound::{WavSpec, WavWriter};
use parking_lot::RwLock;
use serenity::{all::prelude::TypeMapKey, async_trait, model::id::UserId};
use songbird::{Event, EventContext, EventHandler as VoiceEventHandler};

#[derive(Default)]
pub struct VoiceClipRecorderState {
    pub ssrc_to_user: HashMap<u32, UserId>,
    pub user_buffers: HashMap<UserId, UserAudioBuffer>,
}

#[derive(Clone)]
pub struct VoiceClipRecorder {
    state: Arc<RwLock<VoiceClipRecorderState>>,
}

pub struct VCRStateKey;

impl TypeMapKey for VCRStateKey {
    type Value = Arc<RwLock<VoiceClipRecorderState>>;
}

impl VoiceClipRecorder {
    pub fn with_state(state: Arc<RwLock<VoiceClipRecorderState>>) -> Self {
        Self { state }
    }

    fn handle_new_user(&self, ssrc: u32, user_id: UserId) {
        let mut state = self.state.write();
        state.ssrc_to_user.insert(ssrc, user_id);
        state
            .user_buffers
            .entry(user_id)
            .or_insert_with(UserAudioBuffer::new);
    }

    fn handle_client_disconnect(&self, user_id: UserId) {
        let mut state = self.state.write();
        state.user_buffers.remove(&user_id);
        state.ssrc_to_user.retain(|_, v| *v != user_id);
    }
}

#[async_trait]
impl VoiceEventHandler for VoiceClipRecorder {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        match ctx {
            EventContext::SpeakingStateUpdate(speaking) => {
                if let Some(user_id) = speaking.user_id {
                    let user_id = UserId::new(user_id.0);
                    self.handle_new_user(speaking.ssrc, user_id);
                }
            }
            EventContext::ClientDisconnect(disconnect) => {
                let user_id = UserId::new(disconnect.user_id.0);
                self.handle_client_disconnect(user_id);
            }
            EventContext::VoiceTick(tick) => {
                let mut state = self.state.write();
                for (ssrc, voice_data) in &tick.speaking {
                    if let Some(user_id) = state.ssrc_to_user.get(ssrc).copied()
                        && let Some(decoded) = &voice_data.decoded_voice
                        && let Some(buf) = state.user_buffers.get_mut(&user_id)
                    {
                        buf.push_frame(decoded);
                    }
                }
            }
            _ => {}
        }

        None
    }
}

use std::time::Instant;

pub const SAMPLE_RATE: usize = 48_000;
pub const BUFFER_SECONDS: usize = 60;
pub const MAX_SAMPLES: usize = SAMPLE_RATE * BUFFER_SECONDS;

pub struct UserAudioBuffer {
    buffer: Vec<i16>,
    write_idx: usize,
    total_written: usize,
    pub last_seen: Option<Instant>,
}

impl UserAudioBuffer {
    pub fn new() -> Self {
        Self {
            buffer: vec![0i16; MAX_SAMPLES],
            write_idx: 0,
            total_written: 0,
            last_seen: None,
        }
    }

    pub fn push_frame(&mut self, pcm: &[i16]) {
        let now = Instant::now();

        if let Some(last) = self.last_seen {
            let elapsed_ms = now.duration_since(last).as_millis() as usize;

            if elapsed_ms > 30 {
                let missing_samples = ((elapsed_ms - 20) * SAMPLE_RATE) / 1000;
                let silence_to_fill = missing_samples.min(MAX_SAMPLES);
                self.write_silence(silence_to_fill);
            }
        }

        self.write_slice(pcm);
        self.last_seen = Some(now);
    }

    pub fn write_silence(&mut self, count: usize) {
        if count == 0 {
            return;
        }

        let count = count.min(MAX_SAMPLES);
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

    pub fn write_slice(&mut self, mut data: &[i16]) {
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

    pub fn get_linearized_pcm(&self) -> Vec<i16> {
        let mut output = Vec::with_capacity(self.total_written);

        if self.total_written < MAX_SAMPLES {
            output.extend_from_slice(&self.buffer[..self.write_idx]);
        } else {
            output.extend_from_slice(&self.buffer[self.write_idx..]);
            output.extend_from_slice(&self.buffer[..self.write_idx]);
        }

        output
    }
}

pub fn pcm_to_wav_bytes(samples: &[i16]) -> Result<Vec<u8>, hound::Error> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut cursor = io::Cursor::new(Vec::with_capacity(samples.len() * 2 + 44));
    {
        let mut writer = WavWriter::new(&mut cursor, spec)?;
        for &sample in samples {
            writer.write_sample(sample)?;
        }
        writer.finalize()?;
    }

    Ok(cursor.into_inner())
}
