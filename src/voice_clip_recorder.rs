use std::collections::VecDeque;
use std::io::{Cursor, Write};
use std::time::Instant;
use std::{collections::HashMap, io, sync::Arc};

use byteorder::{LittleEndian, WriteBytesExt};
use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use parking_lot::RwLock;
use serenity::{all::prelude::TypeMapKey, async_trait, model::id::UserId};
use songbird::packet::Packet;
use songbird::{Event, EventContext, EventHandler as VoiceEventHandler};

#[derive(Default)]
pub struct VoiceClipRecorderState {
    pub ssrc_map: RwLock<HashMap<u32, UserId>>,
    pub buffers: RwLock<HashMap<UserId, UserOpusBuffer>>,
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
        match ctx {
            EventContext::VoiceTick(tick) => {
                let ssrc_map = self.state.ssrc_map.read();
                let mut buffers = self.state.buffers.write();
                for (ssrc, voice_data) in &tick.speaking {
                    if let Some(uid) = ssrc_map.get(ssrc) {
                        if let Some(rtp_data) = &voice_data.packet {
                            let rtp = rtp_data.rtp();
                            let raw_payload = rtp.payload();

                            // Slicing out the real Opus frame
                            let start = rtp_data.payload_offset;
                            let end = raw_payload.len().saturating_sub(rtp_data.payload_end_pad);

                            if start < end && end <= raw_payload.len() {
                                let opus_frame: &[u8] = &raw_payload[start..end];

                                let buf = buffers.entry(*uid).or_default();
                                buf.push_frame(opus_frame);
                            }
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

pub const CHANNELS: u8 = 2;
pub const MAX_FRAMES: usize = 3000; // 60s * 50 frames/sec (20ms each)
pub const OPUS_SILENCE_FRAME: [u8; 3] = [0xF8, 0xFF, 0xFE];
pub const SAMPLES_PER_FRAME: u64 = 960; // 48kHz * 0.02s
pub const SAMPLE_RATE: u32 = 48_000;

pub struct UserOpusBuffer {
    pub frames: VecDeque<Vec<u8>>,
    pub last_seen: Option<Instant>,
}

impl Default for UserOpusBuffer {
    fn default() -> Self {
        Self {
            frames: VecDeque::with_capacity(MAX_FRAMES),
            last_seen: None,
        }
    }
}

impl UserOpusBuffer {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn push_frame(&mut self, payload: &[u8]) {
        let now = Instant::now();

        if let Some(last) = self.last_seen {
            let elapsed_ms = now.duration_since(last).as_millis() as usize;
            if elapsed_ms > 30 {
                // How many 20ms frames were skipped
                let missing_frames = (elapsed_ms - 20) / 20;
                let fill_count = missing_frames.min(MAX_FRAMES);
                for _ in 0..fill_count {
                    self.push_raw(OPUS_SILENCE_FRAME.to_vec());
                }
            }
        }

        self.push_raw(payload.to_vec());
        self.last_seen = Some(now);
    }

    fn push_raw(&mut self, frame: Vec<u8>) {
        if self.frames.len() >= MAX_FRAMES {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
    }

    /// Muxes all buffered Opus frames into a playable in-memory Ogg Opus file.
    pub fn to_ogg_bytes(&self) -> io::Result<Vec<u8>> {
        if self.frames.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "No audio recorded",
            ));
        }

        let stream_serial: u32 = 1337;
        let mut out = Cursor::new(Vec::with_capacity(self.frames.len() * 120));
        let mut writer = PacketWriter::new(&mut out);

        write_ogg_opus_headers(&mut writer, stream_serial)?;

        let mut granule_pos: u64 = 0;
        let total = self.frames.len();

        for (i, frame) in self.frames.iter().enumerate() {
            granule_pos += SAMPLES_PER_FRAME;

            let end_info = if i == total - 1 {
                PacketWriteEndInfo::EndStream
            } else if (i + 1) % 50 == 0 {
                PacketWriteEndInfo::EndPage
            } else {
                PacketWriteEndInfo::NormalPacket
            };

            writer.write_packet(frame.as_slice(), stream_serial, end_info, granule_pos)?;
        }

        Ok(out.into_inner())
    }
}

fn write_ogg_opus_headers<W: Write>(writer: &mut PacketWriter<W>, serial: u32) -> io::Result<()> {
    // OpusHead Packet
    let mut head = Vec::with_capacity(19);
    head.extend_from_slice(b"OpusHead");
    head.push(1); // Version
    head.push(CHANNELS);
    head.write_u16::<LittleEndian>(0)?; // Pre-skip (0 for live raw captures)
    head.write_u32::<LittleEndian>(SAMPLE_RATE)?;
    head.write_i16::<LittleEndian>(0)?; // Output gain (0 dB)
    head.push(0); // Channel mapping family (0 = mono / stereo)

    writer.write_packet(head, serial, PacketWriteEndInfo::EndPage, 0)?;

    // OpusTags Packet
    let vendor = "Voice Clip Recorder";
    let mut tags = Vec::with_capacity(16 + vendor.len());
    tags.extend_from_slice(b"OpusTags");
    tags.write_u32::<LittleEndian>(vendor.len() as u32)?;
    tags.extend_from_slice(vendor.as_bytes());
    tags.write_u32::<LittleEndian>(0)?; // User comments list length (0 comments)

    writer.write_packet(tags, serial, PacketWriteEndInfo::EndPage, 0)?;

    Ok(())
}
