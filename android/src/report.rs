//! Log stream bergaya Moonlight untuk aplikasi CloudPC. Format baris dan blok "Global video stats" SAMA dengan client
//! desktop (`client/src/main.rs`), karena aplikasi membacanya dengan `parse_stream_log` yang sama untuk dikirim ke server.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use cloudpc_stream_client::session::{Event, StreamReport};
use cloudpc_stream_client::video::DisplayReport;

pub struct Log {
    file: Option<File>,
    started: Instant,
}

impl Log {
    /// Log ke `path` (ditimpa). Gagal membuka file tidak menggagalkan stream; baris dibuang saja.
    pub fn create(path: &Path) -> Log {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Log { file: File::create(path).ok(), started: Instant::now() }
    }

    /// Satu baris `HH:MM:SS - CloudPC Info: ...` (waktu sejak stream dibuka).
    pub fn line(&mut self, text: &str) {
        let t = self.started.elapsed().as_secs();
        self.raw(&format!("{:02}:{:02}:{:02} - CloudPC Info: {text}", t / 3600, t / 60 % 60, t % 60));
    }

    pub fn raw(&mut self, text: &str) {
        if let Some(file) = self.file.as_mut() {
            let _ = writeln!(file, "{text}");
            let _ = file.flush();
        }
    }
}

/// Baris log untuk kejadian sesi (sama dengan `event_logger` client desktop).
pub fn event_lines(event: &Event, renderer: &str) -> Vec<String> {
    match event {
        Event::Negotiated { width, height, fps, video_format, bitrate_kbps } => vec![
            format!("Video stream is {width}x{height}x{fps} (format {video_format:#x})"),
            format!("Video bitrate: {bitrate_kbps} kbps"),
            format!("Renderer '{renderer}' chosen"),
        ],
        Event::FirstVideoPacket(after) => vec![format!("Received first video packet after {} ms", after.as_millis())],
        Event::RfiRequest(frame) => vec![format!("Sending RFI request for unrecoverable frame {frame}")],
        Event::FirstRumble { pad, low, high } => vec![format!("Getar gamepad {pad} dari host (motor {low}/{high})")],
    }
}

/// Blok "Global video stats" persis format Moonlight 6.2.
pub fn global_stats(report: &StreamReport, shown: Option<&DisplayReport>) -> String {
    let seconds = report.duration.saturating_sub(report.connected_after).as_secs_f64().max(0.001);
    let v = report.video;
    let received = v.frames as f64 / seconds;
    let (decoded, rendered, decode_ms, queue_ms, render_ms, skipped) = match shown {
        Some(s) => {
            let queue = (s.receive_to_present_ms - s.decode_ms - s.render_ms).max(0.0);
            (s.decoded as f64 / seconds, s.presented as f64 / seconds, s.decode_ms, queue, s.render_ms, s.skipped)
        }
        None => (received, received, 0.0, 0.0, 0.0, 0),
    };
    let dropped_network = if v.frames + v.frames_lost > 0 { v.frames_lost as f64 * 100.0 / (v.frames + v.frames_lost) as f64 } else { 0.0 };
    let dropped_jitter = if v.frames > 0 { skipped as f64 * 100.0 / v.frames as f64 } else { 0.0 };
    let (low, high) = report.host_latency_range_ms.unwrap_or((0.0, 0.0));
    let (rtt, variance) = report.network_rtt_ms.unwrap_or((0.0, 0.0));
    [
        String::new(),
        "Global video stats".into(),
        "-------------------".into(),
        format!("Incoming frame rate from network: {received:.2} FPS"),
        format!("Decoding frame rate: {decoded:.2} FPS"),
        format!("Rendering frame rate: {rendered:.2} FPS"),
        format!("Host processing latency min/max/average: {low:.1}/{high:.1}/{:.1} ms", report.host_latency_ms.unwrap_or(0.0)),
        format!("Frames dropped by your network connection: {dropped_network:.2}%"),
        format!("Frames dropped due to network jitter: {dropped_jitter:.2}%"),
        format!("Average network latency: {rtt:.0} ms (variance: {variance:.0} ms)"),
        format!("Average decoding time: {decode_ms:.2} ms"),
        format!("Average frame queue delay: {queue_ms:.2} ms"),
        format!("Average rendering time (including monitor V-sync latency): {render_ms:.2} ms"),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn stats_block_has_the_lines_the_app_parses() {
        let mut report = StreamReport { duration: Duration::from_secs(11), connected_after: Duration::from_secs(1), ..Default::default() };
        report.video.frames = 600;
        report.video.frames_lost = 6;
        report.host_latency_ms = Some(1.5);
        report.host_latency_range_ms = Some((1.0, 4.2));
        report.network_rtt_ms = Some((24.4, 3.0));
        let shown = DisplayReport { decoded: 598, presented: 590, decode_ms: 4.25, receive_to_present_ms: 6.0, ..Default::default() };
        let text = global_stats(&report, Some(&shown));
        for expected in [
            "Global video stats\n-------------------",
            "Incoming frame rate from network: 60.00 FPS",
            "Decoding frame rate: 59.80 FPS",
            "Rendering frame rate: 59.00 FPS",
            "Host processing latency min/max/average: 1.0/4.2/1.5 ms",
            "Frames dropped by your network connection: 0.99%",
            "Average network latency: 24 ms (variance: 3 ms)",
            "Average decoding time: 4.25 ms",
            "Average frame queue delay: 1.75 ms",
        ] {
            assert!(text.contains(expected), "tidak ada `{expected}` di:\n{text}");
        }
    }

    #[test]
    fn log_lines_look_like_moonlight_and_reach_the_file() {
        let dir = std::env::temp_dir().join(format!("cloudpc-stream-android-log-{}", std::process::id()));
        let path = dir.join("logs").join("moonlight-stream.log");
        let mut log = Log::create(&path);
        for line in event_lines(&Event::Negotiated { width: 1280, height: 720, fps: 60, video_format: 0x100, bitrate_kbps: 8000 }, "MediaCodec") {
            log.line(&line);
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("00:00:00 - CloudPC Info: Video stream is 1280x720x60 (format 0x100)"), "{text}");
        assert!(text.contains("Video bitrate: 8000 kbps"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
