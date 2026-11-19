//! Keadaan satu sesi stream di host: dari `/launch` (HTTPS) lewat RTSP sampai kanal kontrol tersambung.
//! Padanan `launch_session_t` (`rtsp.h`) dan `session_t`/`config_t` (`stream.cpp`, `stream.h`) di Sunshine.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use cloudpc_stream_proto::control::{client_feature, encryption};

/// Sesi peluncuran yang belum diklaim kanal kontrol dibuang setelah ini (bawaan `ping_timeout` Sunshine).
pub const LAUNCH_TTL: Duration = Duration::from_secs(10);

/// Dibuat `/launch` atau `/resume`; dipakai RTSP untuk kunci enkripsi dan data sambungan.
pub struct LaunchSession {
    pub id: u32,
    /// `rikey` dari client: kunci AES-128 untuk RTSP, kanal kontrol, video, dan audio sesi ini.
    pub key: [u8; 16],
    /// `rikeyid`: awalan IV audio (dipakai mulai fase H3).
    pub key_id: u32,
    pub client_name: String,
    /// 16 karakter yang harus dikirim client di ping UDP video/audio.
    pub ping_payload: String,
    /// Angka yang harus dibawa client saat menyambung kanal kontrol.
    pub connect_data: u32,
    /// Penghitung IV balasan RTSP dari host.
    pub rtsp_seq: u32,
    pub created: Instant,
}

impl LaunchSession {
    pub fn expired(&self) -> bool {
        self.created.elapsed() > LAUNCH_TTL
    }
}

/// Pilihan client dari SDP di RTSP ANNOUNCE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    /// Ukuran payload satu paket video.
    pub packet_size: u32,
    /// 0 = H.264, 1 = HEVC, 2 = AV1.
    pub video_format: u32,
    pub audio_channels: u32,
    /// Lama satu paket audio dalam milidetik (5 atau 10 dari Moonlight).
    pub audio_packet_ms: u32,
    pub encryption_flags: u32,
    pub client_feature_flags: u32,
    pub min_fec_packets: u32,
    pub slices_per_frame: u32,
    pub ref_frames: u32,
}

/// Mengapa ANNOUNCE ditolak.
#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// Atribut wajib tidak ada atau nilainya di luar batas: balas 400.
    Bad(String),
    /// Client tidak menyalakan enkripsi yang diwajibkan host: balas 403.
    EncryptionRequired,
}

impl StreamConfig {
    pub fn from_sdp(attrs: &HashMap<String, String>) -> Result<StreamConfig, ConfigError> {
        let get = |name: &str, default: Option<u32>, range: std::ops::RangeInclusive<u32>| -> Result<u32, ConfigError> {
            let value = match attrs.get(name) {
                Some(text) => text.parse::<u32>().map_err(|_| ConfigError::Bad(format!("{name} bukan angka")))?,
                None => default.ok_or_else(|| ConfigError::Bad(format!("{name} tidak ada")))?,
            };
            if !range.contains(&value) {
                return Err(ConfigError::Bad(format!("{name}={value} di luar batas")));
            }
            Ok(value)
        };
        let any = 0..=u32::MAX;
        let mut encryption_flags = get("x-ss-general.encryptionEnabled", Some(0), any.clone())?;
        // Bit 0x20 di bendera fitur NVIDIA = client meminta audio terenkripsi (cara lama).
        if get("x-nv-general.featureFlags", Some(135), any.clone())? & 0x20 != 0 {
            encryption_flags |= encryption::AUDIO;
        }
        let config = StreamConfig {
            width: get("x-nv-video[0].clientViewportWd", None, 64..=8192)?,
            height: get("x-nv-video[0].clientViewportHt", None, 64..=8192)?,
            fps: get("x-nv-video[0].maxFPS", None, 1..=480)?,
            bitrate_kbps: get("x-nv-vqos[0].bw.maximumBitrateKbps", None, 100..=1_000_000)?,
            packet_size: get("x-nv-video[0].packetSize", None, 200..=2048)?,
            video_format: get("x-nv-vqos[0].bitStreamFormat", Some(0), 0..=2)?,
            audio_channels: get("x-nv-audio.surround.numChannels", None, 2..=8)?,
            audio_packet_ms: get("x-nv-aqos.packetDuration", Some(5), 5..=20)?,
            encryption_flags,
            client_feature_flags: get("x-ml-general.featureFlags", Some(0), any.clone())?,
            min_fec_packets: get("x-nv-vqos[0].fec.minRequiredFecPackets", Some(0), 0..=255)?,
            slices_per_frame: get("x-nv-video[0].videoEncoderSlicesPerFrame", None, 1..=64)?,
            ref_frames: get("x-nv-video[0].maxNumReferenceFrames", None, any)?,
        };
        // Host baru punya encoder stereo; surround akan menghasilkan suara rusak di client, jadi ditolak dengan jelas.
        if config.audio_channels != 2 {
            return Err(ConfigError::Bad(format!("audio {} kanal belum didukung (hanya stereo)", config.audio_channels)));
        }
        // Opus hanya menerima frame 5, 10, atau 20 ms di rentang ini.
        if ![5, 10, 20].contains(&config.audio_packet_ms) {
            return Err(ConfigError::Bad(format!("lama paket audio {} ms tidak didukung", config.audio_packet_ms)));
        }
        // Stream produksi lewat internet (VPS), jadi kontrol, video, dan audio wajib terenkripsi; tanpa pengecualian LAN.
        let required = encryption::CONTROL_V2 | encryption::VIDEO | encryption::AUDIO;
        if config.encryption_flags & required != required {
            return Err(ConfigError::EncryptionRequired);
        }
        // Tanpa ini sesi kontrol hanya bisa dicocokkan lewat alamat IP, yang sama untuk semua pemain di balik VPS.
        if config.client_feature_flags & client_feature::SESSION_ID_V1 == 0 {
            return Err(ConfigError::Bad("client terlalu lama: tidak mendukung X-SS-Connect-Data".into()));
        }
        Ok(config)
    }
}

/// Sesi stream yang sudah dinegosiasikan (ANNOUNCE diterima).
pub struct StreamSession {
    pub launch_id: u32,
    pub key: [u8; 16],
    /// `rikeyid`: dasar IV enkripsi audio.
    pub key_id: u32,
    pub connect_data: u32,
    /// 16 karakter yang harus ada di ping UDP video/audio dari client.
    pub ping_payload: String,
    /// Alamat UDP tujuan video, diketahui dari ping client.
    pub video_client: Option<std::net::SocketAddr>,
    /// Alamat UDP tujuan audio, diketahui dari ping client ke port audio.
    pub audio_client: Option<std::net::SocketAddr>,
    pub client_name: String,
    pub config: StreamConfig,
    /// Indeks peer ENet setelah kanal kontrol tersambung.
    pub peer: Option<usize>,
    /// Penghitung IV pesan kontrol dari host.
    pub out_seq: u32,
    /// Jumlah pesan kontrol sah yang diterima, per tipe.
    pub messages: BTreeMap<u16, u32>,
}

/// Salinan keadaan sesi untuk log dan tes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamSnapshot {
    pub client_name: String,
    pub config: StreamConfig,
    pub control_connected: bool,
    pub messages: BTreeMap<u16, u32>,
}

impl StreamSession {
    pub fn snapshot(&self) -> StreamSnapshot {
        StreamSnapshot { client_name: self.client_name.clone(), config: self.config.clone(), control_connected: self.peer.is_some(), messages: self.messages.clone() }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Atribut SDP minimal seperti yang dikirim Moonlight 6.2 (enkripsi penuh, sesi ber-ID).
    pub(crate) fn moonlight_sdp() -> HashMap<String, String> {
        [
            ("x-nv-video[0].clientViewportWd", "1920"),
            ("x-nv-video[0].clientViewportHt", "1080"),
            ("x-nv-video[0].maxFPS", "60"),
            ("x-nv-video[0].packetSize", "1392"),
            ("x-nv-vqos[0].bw.maximumBitrateKbps", "20000"),
            ("x-nv-vqos[0].bitStreamFormat", "1"),
            ("x-nv-audio.surround.numChannels", "2"),
            ("x-nv-video[0].videoEncoderSlicesPerFrame", "1"),
            ("x-nv-video[0].maxNumReferenceFrames", "0"),
            ("x-ss-general.encryptionEnabled", "7"),
            ("x-ml-general.featureFlags", "3"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn parses_full_config() {
        let config = StreamConfig::from_sdp(&moonlight_sdp()).unwrap();
        assert_eq!((config.width, config.height, config.fps, config.video_format), (1920, 1080, 60, 1));
        assert_eq!((config.bitrate_kbps, config.packet_size, config.encryption_flags), (20000, 1392, 7));
    }

    #[test]
    fn missing_or_absurd_values_are_bad_requests() {
        let mut attrs = moonlight_sdp();
        attrs.remove("x-nv-video[0].packetSize");
        assert!(matches!(StreamConfig::from_sdp(&attrs), Err(ConfigError::Bad(_))));
        for (name, value) in [("x-nv-video[0].clientViewportWd", "0"), ("x-nv-video[0].clientViewportHt", "99999"), ("x-nv-video[0].maxFPS", "abc"), ("x-nv-video[0].packetSize", "65000"), ("x-nv-vqos[0].bitStreamFormat", "9")] {
            let mut attrs = moonlight_sdp();
            attrs.insert(name.to_string(), value.to_string());
            assert!(matches!(StreamConfig::from_sdp(&attrs), Err(ConfigError::Bad(_))), "{name}={value}");
        }
    }

    #[test]
    fn unencrypted_or_idless_clients_are_refused() {
        for flags in ["0", "1", "3", "5", "6"] {
            let mut attrs = moonlight_sdp();
            attrs.insert("x-ss-general.encryptionEnabled".into(), flags.into());
            assert_eq!(StreamConfig::from_sdp(&attrs), Err(ConfigError::EncryptionRequired), "bendera {flags}");
        }
        let mut attrs = moonlight_sdp();
        attrs.insert("x-ml-general.featureFlags".into(), "1".into());
        assert!(matches!(StreamConfig::from_sdp(&attrs), Err(ConfigError::Bad(_))));
    }

    #[test]
    fn only_stereo_and_valid_opus_frame_lengths_are_accepted() {
        assert_eq!(StreamConfig::from_sdp(&moonlight_sdp()).unwrap().audio_packet_ms, 5, "bawaan 5 ms");
        for (name, value, ok) in [("x-nv-aqos.packetDuration", "10", true), ("x-nv-aqos.packetDuration", "7", false), ("x-nv-aqos.packetDuration", "40", false), ("x-nv-audio.surround.numChannels", "6", false)] {
            let mut attrs = moonlight_sdp();
            attrs.insert(name.to_string(), value.to_string());
            assert_eq!(StreamConfig::from_sdp(&attrs).is_ok(), ok, "{name}={value}");
        }
    }

    #[test]
    fn legacy_audio_encryption_bit_counts() {
        let mut attrs = moonlight_sdp();
        attrs.insert("x-ss-general.encryptionEnabled".into(), "3".into());
        attrs.insert("x-nv-general.featureFlags".into(), "167".into());
        assert_eq!(StreamConfig::from_sdp(&attrs).unwrap().encryption_flags, 7);
    }
}
