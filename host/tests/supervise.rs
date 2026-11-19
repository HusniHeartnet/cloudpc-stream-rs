//! Pengawas (`cloudpc-stream-host supervise`) dalam mode uji: tes berjalan sebagai pengembang, bukan SYSTEM, jadi
//! host dijalankan di sesi ini. Yang diuji: host hidup, dijalankan ulang setelah mati, ikut mati bersama pengawas,
//! dan log masuk ke folder state. Jalur SYSTEM (pindah sesi) tidak bisa diuji dari sini.
#![cfg(windows)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const HOST: &str = env!("CARGO_BIN_EXE_cloudpc-stream-host");

fn serverinfo(port: u16) -> bool {
    let Ok(mut stream) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300)) else { return false };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.write_all(b"GET /serverinfo HTTP/1.1\r\nHost: x\r\n\r\n");
    let mut body = String::new();
    let _ = stream.read_to_string(&mut body);
    body.contains("<uniqueid>")
}

fn wait_for(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}

/// Semua pid host yang pernah tercatat di log host.
fn host_pids(state: &Path) -> Vec<u32> {
    let log = std::fs::read_to_string(state.join("host.log")).unwrap_or_default();
    log.lines().filter_map(|line| line.split("(pid ").nth(1)?.split(')').next()?.parse().ok()).collect()
}

struct Supervisor(Child);

impl Drop for Supervisor {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn supervisor_restarts_a_dead_host_and_takes_it_down_when_it_exits() {
    let state = tempfile::tempdir().unwrap();
    // Rentang port sendiri supaya tidak bertabrakan dengan tes e2e yang berjalan bersamaan.
    let base = 41000 + (std::process::id() % 500) as u16 * 10;
    let supervisor = Command::new(HOST)
        .args(["supervise", "--state"])
        .arg(state.path())
        .args(["--base-port", &base.to_string(), "--test-silent-audio", "--test-record-input"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut supervisor = Supervisor(supervisor);

    assert!(wait_for(Duration::from_secs(20), || serverinfo(base)), "host harus hidup lewat pengawas");
    let first = *host_pids(state.path()).last().expect("pid host tercatat di host.log");

    // Host mati mendadak (crash): pengawas harus menjalankannya lagi.
    assert!(Command::new("taskkill").args(["/PID", &first.to_string(), "/F"]).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success());
    assert!(wait_for(Duration::from_secs(5), || !serverinfo(base)), "host lama harus benar-benar mati");
    assert!(wait_for(Duration::from_secs(20), || serverinfo(base) && host_pids(state.path()).last() != Some(&first)), "pengawas harus menjalankan host baru");
    assert_eq!(host_pids(state.path()).len(), 2);

    let log = std::fs::read_to_string(state.path().join("supervisor.log")).unwrap();
    assert!(log.contains("bukan SYSTEM"), "mode uji harus diperingatkan: {log}");
    assert!(log.matches("host dijalankan").count() == 2 && log.contains("host berhenti"), "{log}");

    // Pengawas hilang: host tidak boleh tertinggal memegang port.
    supervisor.0.kill().unwrap();
    supervisor.0.wait().unwrap();
    assert!(wait_for(Duration::from_secs(10), || !serverinfo(base)), "host harus berhenti saat pengawasnya hilang");
}

#[test]
#[ignore = "mengubah ACL folder sementara; butuh shell administrator supaya foldernya bisa dihapus lagi"]
fn state_folder_is_locked_to_system_and_administrators() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("host_key.pem"), b"rahasia").unwrap();
    cloudpc_stream_host::supervise::lock_state_dir(dir.path()).unwrap();

    // SID pemegang hak, dan apakah pewarisan dari folder induk sudah diputus.
    let script = format!(
        "$a = Get-Acl -LiteralPath '{}'; ($a.Access | ForEach-Object {{ $_.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value }} | Sort-Object -Unique) -join ','; $a.AreAccessRulesProtected",
        dir.path().display()
    );
    let output = Command::new("powershell").args(["-NoProfile", "-Command", &script]).output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    assert_eq!(lines.next(), Some("S-1-5-18,S-1-5-32-544"), "hanya SYSTEM dan Administrators: {text}");
    assert_eq!(lines.next(), Some("True"), "pewarisan harus diputus: {text}");
}
