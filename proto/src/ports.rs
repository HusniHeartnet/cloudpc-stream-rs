//! Peta port GameStream: semua port dihitung dari satu port dasar (`nvhttp.h`, `rtsp.h`, `stream.h` di Sunshine).
//!
//! Port admin Sunshine (dasar + 1 = 47990) sengaja TIDAK ada di sini: stack ini tidak punya Web UI.

/// Port dasar bawaan (HTTP tanpa TLS: `/serverinfo` dan `/pair`).
pub const BASE_PORT: u16 = 47989;

pub const OFFSET_HTTPS: i32 = -5;
pub const OFFSET_HTTP: i32 = 0;
pub const OFFSET_VIDEO: i32 = 9;
pub const OFFSET_CONTROL: i32 = 10;
pub const OFFSET_AUDIO: i32 = 11;
pub const OFFSET_RTSP: i32 = 21;

/// Port yang dipakai satu host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ports {
    /// TCP, HTTPS dengan sertifikat klien (applist, launch, resume, cancel).
    pub https: u16,
    /// TCP, HTTP polos (serverinfo, pair).
    pub http: u16,
    /// UDP, video RTP.
    pub video: u16,
    /// UDP, kanal kontrol (ENet).
    pub control: u16,
    /// UDP, audio RTP.
    pub audio: u16,
    /// TCP, RTSP (negosiasi sesi).
    pub rtsp: u16,
}

impl Ports {
    /// `None` bila salah satu port keluar dari rentang 1..=65535.
    pub fn from_base(base: u16) -> Option<Ports> {
        let at = |offset: i32| -> Option<u16> {
            let port = i32::from(base) + offset;
            u16::try_from(port).ok().filter(|p| *p != 0)
        };
        Some(Ports {
            https: at(OFFSET_HTTPS)?,
            http: at(OFFSET_HTTP)?,
            video: at(OFFSET_VIDEO)?,
            control: at(OFFSET_CONTROL)?,
            audio: at(OFFSET_AUDIO)?,
            rtsp: at(OFFSET_RTSP)?,
        })
    }
}

impl Default for Ports {
    fn default() -> Self {
        Ports::from_base(BASE_PORT).expect("port dasar bawaan valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ports_match_sunshine() {
        // Sama dengan port stream yang diteruskan relay.
        let p = Ports::default();
        assert_eq!((p.https, p.http, p.video, p.control, p.audio, p.rtsp), (47984, 47989, 47998, 47999, 48000, 48010));
    }

    #[test]
    fn out_of_range_base_is_rejected() {
        assert!(Ports::from_base(3).is_none());
        assert!(Ports::from_base(65530).is_none());
    }
}
