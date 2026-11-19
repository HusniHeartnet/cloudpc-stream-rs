//! Fase C1. Percakapan HTTP/HTTPS dengan host GameStream (Sunshine atau host CloudPC Rust).
//!
//! Rujukan: `moonlight-qt/app/backend/nvhttp.cpp`, `nvpairingmanager.cpp`. Beda penting dari Moonlight: client
//! TIDAK mengingat alamat host. Setiap panggilan memakai persis alamat yang diberikan; host dikenali lewat `uniqueid`
//! di `/serverinfo` hanya untuk memilih sertifikat yang dikunci.

use std::time::Duration;

use cloudpc_stream_common::crypto::{ecb_decrypt, ecb_encrypt, hex_decode, hex_encode, sha256, sign_sha256, ParsedCert};
use cloudpc_stream_proto::pairing::derive_aes_key;
use rand::RngCore;

use crate::http::{self, tag, Tls};
use crate::store::{KnownHost, Store};

const QUICK: Duration = Duration::from_secs(10);
/// Langkah pertama pairing menunggu PIN dimasukkan di host (oleh agent, atau orang di host).
const PIN_WAIT: Duration = Duration::from_secs(150);
const LAUNCH_WAIT: Duration = Duration::from_secs(30);

/// Alamat host persis seperti yang diberikan pemanggil: nama/IP + port HTTP (bawaan 47989).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostAddr {
    pub host: String,
    pub http_port: u16,
}

impl HostAddr {
    pub fn parse(text: &str) -> Result<HostAddr, String> {
        let (host, http_port) = http::split_host_port(text, cloudpc_stream_proto::ports::BASE_PORT)?;
        Ok(HostAddr { host, http_port })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    pub hostname: String,
    pub uniqueid: String,
    pub https_port: u16,
    pub app_version: String,
    /// `ServerCodecModeSupport`.
    pub codecs: u32,
    pub current_game: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub id: u32,
    pub title: String,
}

/// Hasil `/launch` atau `/resume`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launched {
    /// `rtsp://...` atau `rtspenc://...` (RTSP terenkripsi).
    pub session_url: String,
    pub key: [u8; 16],
    pub key_id: u32,
    pub resumed: bool,
}

fn random16() -> [u8; 16] {
    let mut b = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut b);
    b
}

/// Bagian query yang dikirim di setiap permintaan, seperti Moonlight.
fn base_query(store: &Store) -> String {
    format!("uniqueid={}&uuid={}", store.uniqueid(), hex_encode(&random16()))
}

/// Balasan dengan `status_code` bukan 200 jadi galat berisi pesan host.
fn check(what: &str, xml: &str) -> Result<(), String> {
    match http::status(xml) {
        (200, _) => Ok(()),
        (0, _) => Err(format!("{what}: balasan host tidak dikenali")),
        (code, message) => Err(format!("{what} ditolak host ({code}{})", if message.is_empty() { String::new() } else { format!(": {message}") })),
    }
}

fn parse_serverinfo(xml: &str) -> Result<ServerInfo, String> {
    check("serverinfo", xml)?;
    let number = |name: &str| tag(xml, name).and_then(|v| v.parse::<u64>().ok());
    Ok(ServerInfo {
        hostname: tag(xml, "hostname").unwrap_or_default(),
        uniqueid: tag(xml, "uniqueid").filter(|u| !u.is_empty()).ok_or("host tidak menyebut uniqueid")?,
        https_port: number("HttpsPort").and_then(|p| u16::try_from(p).ok()).ok_or("host tidak menyebut HttpsPort")?,
        app_version: tag(xml, "appversion").unwrap_or_default(),
        codecs: number("ServerCodecModeSupport").unwrap_or(1) as u32,
        current_game: number("currentgame").unwrap_or(0) as u32,
    })
}

/// `/serverinfo` lewat HTTP polos (tanpa pairing).
pub fn server_info(addr: &HostAddr, store: &Store) -> Result<ServerInfo, String> {
    let (_, xml) = http::get(&addr.host, addr.http_port, &format!("/serverinfo?{}", base_query(store)), None, QUICK)?;
    parse_serverinfo(&xml)
}

/// Host ini sudah dipairing dengan client ini? Mengembalikan info + DER sertifikat host yang dikunci.
pub fn paired_host(addr: &HostAddr, store: &Store) -> Result<(ServerInfo, Vec<u8>), String> {
    let info = server_info(addr, store)?;
    let known = store.host(&info.uniqueid).ok_or_else(|| format!("belum dipairing dengan host {} ({})", info.hostname, info.uniqueid))?;
    let cert = ParsedCert::from_pem(known.cert_pem.as_bytes())?;
    Ok((info, cert.der))
}

fn https_get(addr: &HostAddr, info: &ServerInfo, cert_der: &[u8], store: &Store, path: &str, query: &str, timeout: Duration) -> Result<String, String> {
    let sep = if query.is_empty() { "" } else { "&" };
    let target = format!("{path}?{}{sep}{query}", base_query(store));
    let (code, xml) = http::get(&addr.host, info.https_port, &target, Some(Tls { identity: &store.identity, host_cert_der: cert_der }), timeout)?;
    if code == 401 {
        return Err("host tidak lagi mengenali perangkat ini (pairing dicabut)".into());
    }
    Ok(xml)
}

/// Pairing dengan PIN. Mengembalikan nama host. Galat yang jelas bila PIN di host tidak sama.
pub fn pair(addr: &HostAddr, store: &mut Store, pin: &str) -> Result<String, String> {
    if pin.len() != 4 || !pin.bytes().all(|b| b.is_ascii_digit()) {
        return Err("PIN harus 4 angka".into());
    }
    let info = server_info(addr, store)?;
    let query = format!("{}&devicename=roth&updateState=1", base_query(store));
    let step = |extra: String, timeout: Duration| -> Result<String, String> {
        let (_, xml) = http::get(&addr.host, addr.http_port, &format!("/pair?{query}&{extra}"), None, timeout)?;
        check("pairing", &xml)?;
        if tag(&xml, "paired").as_deref() != Some("1") {
            return Err("pairing ditolak host".into());
        }
        Ok(xml)
    };

    // 1. Sertifikat klien + salt; host menunggu PIN lalu membalas sertifikatnya.
    let salt = random16();
    let xml = step(format!("phrase=getservercert&salt={}&clientcert={}", hex_encode(&salt), hex_encode(store.identity.cert_pem.as_bytes())), PIN_WAIT)?;
    let server_pem = tag(&xml, "plaincert").and_then(|h| hex_decode(&h)).ok_or("host tidak mengirim sertifikat")?;
    let server_cert = ParsedCert::from_pem(&server_pem)?;
    let key = derive_aes_key(&salt, pin);

    // 2. Tantangan klien; host membalas hash bukti + tantangannya sendiri.
    let challenge = random16();
    let xml = step(format!("clientchallenge={}", hex_encode(&ecb_encrypt(&key, &challenge)?)), QUICK)?;
    let response = tag(&xml, "challengeresponse").and_then(|h| hex_decode(&h)).ok_or("balasan tantangan tidak ada")?;
    let response = ecb_decrypt(&key, &response)?;
    if response.len() < 48 {
        return Err("balasan tantangan terlalu pendek".into());
    }
    let (host_hash, server_challenge) = (&response[..32], &response[32..48]);

    // 3. Hash klien; host membuka rahasianya beserta tanda tangan.
    let client_secret = random16();
    let client_hash = sha256(&[server_challenge, &store.identity.cert.signature, &client_secret]);
    let xml = step(format!("serverchallengeresp={}", hex_encode(&ecb_encrypt(&key, &client_hash)?)), QUICK)?;
    let pairing_secret = tag(&xml, "pairingsecret").and_then(|h| hex_decode(&h)).filter(|s| s.len() > 16).ok_or("rahasia host tidak ada")?;
    let (server_secret, server_signature) = pairing_secret.split_at(16);
    // Host harus membuktikan tahu PIN yang sama DAN memegang kunci sertifikat yang dikirimnya.
    let host_knows_pin = sha256(&[&challenge, &server_cert.signature, server_secret])[..] == *host_hash;
    if !host_knows_pin || !server_cert.verify_sha256(server_secret, server_signature) {
        return Err("PIN salah atau host tidak sah (bukti dari host tidak cocok)".into());
    }

    // 4. Rahasia klien + tanda tangan; host mencatat perangkat bila semuanya cocok.
    let signature = sign_sha256(&store.identity.key, &client_secret);
    step(format!("clientpairingsecret={}", hex_encode(&[&client_secret[..], &signature].concat())), QUICK).map_err(|_| "host menolak bukti dari perangkat ini (PIN salah?)".to_string())?;

    // 5. Konfirmasi lewat HTTPS dengan sertifikat yang baru dikunci.
    let xml = https_get(addr, &info, &server_cert.der, store, "/pair", "devicename=roth&updateState=1&phrase=pairchallenge", QUICK)?;
    if tag(&xml, "paired").as_deref() != Some("1") {
        return Err("konfirmasi pairing lewat HTTPS gagal".into());
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    store.remember(&info.uniqueid, KnownHost { name: info.hostname.clone(), cert_pem: String::from_utf8_lossy(&server_pem).into_owned(), paired_at: now })?;
    Ok(info.hostname)
}

fn parse_apps(xml: &str) -> Vec<App> {
    xml.split("<App>")
        .skip(1)
        .filter_map(|chunk| {
            let chunk = chunk.split("</App>").next()?;
            Some(App { id: tag(chunk, "ID")?.parse().ok()?, title: tag(chunk, "AppTitle").unwrap_or_default() })
        })
        .collect()
}

pub fn apps(addr: &HostAddr, store: &Store) -> Result<Vec<App>, String> {
    let (info, cert) = paired_host(addr, store)?;
    let xml = https_get(addr, &info, &cert, store, "/applist", "", QUICK)?;
    check("applist", &xml)?;
    Ok(parse_apps(&xml))
}

/// Mulai (atau lanjutkan) aplikasi `app_title` dengan resolusi/FPS tertentu.
/// Pilihan peluncuran tambahan Moonlight.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaunchExtras {
    /// `--game-optimization`: host boleh mengubah setelan game supaya cocok dengan stream (`sops=1`).
    pub game_optimization: bool,
    /// `--audio-on-host`: suara tetap diputar di speaker host (`localAudioPlayMode=1`).
    pub audio_on_host: bool,
}

/// Parameter `/launch` dan `/resume`, sama dengan Moonlight (`continuousAudio=1` ekstensi Sunshine, lihat atas).
fn launch_query(app_id: u32, (width, height, fps): (u32, u32, u32), key: &[u8; 16], key_id: u32, extras: LaunchExtras) -> String {
    format!(
        "appid={app_id}&mode={width}x{height}x{fps}&additionalStates=1&sops={}&rikey={}&rikeyid={}&localAudioPlayMode={}&surroundAudioInfo=196610&remoteControllersBitmap=0&gcmap=0&gcpersist=0&continuousAudio=1&corever=1",
        u8::from(extras.game_optimization),
        hex_encode(key),
        key_id as i32,
        u8::from(extras.audio_on_host),
    )
}

pub fn launch(addr: &HostAddr, store: &Store, app_title: &str, width: u32, height: u32, fps: u32) -> Result<(ServerInfo, Launched), String> {
    launch_with(addr, store, app_title, width, height, fps, LaunchExtras::default())
}

pub fn launch_with(addr: &HostAddr, store: &Store, app_title: &str, width: u32, height: u32, fps: u32, extras: LaunchExtras) -> Result<(ServerInfo, Launched), String> {
    let (info, cert) = paired_host(addr, store)?;
    let xml = https_get(addr, &info, &cert, store, "/applist", "", QUICK)?;
    check("applist", &xml)?;
    let app = parse_apps(&xml).into_iter().find(|a| a.title.eq_ignore_ascii_case(app_title)).ok_or_else(|| format!("aplikasi \"{app_title}\" tidak ada di host"))?;

    // Lewat HTTPS host melaporkan aplikasi yang sedang berjalan.
    let state = https_get(addr, &info, &cert, store, "/serverinfo", "", QUICK)?;
    let running = tag(&state, "currentgame").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    if running != 0 && running != app.id {
        return Err("aplikasi lain sedang berjalan di host; tutup dulu (quit)".into());
    }
    let resumed = running == app.id;

    let key = random16();
    let key_id = rand::rngs::OsRng.next_u32();
    // continuousAudio=1: Sunshine mengirim hening saat tidak ada suara (tanpa itu aliran audio kosong sampai ada
    // suara). Host CloudPC selalu mengirim hening; host lain mengabaikan parameter ini.
    let query = launch_query(app.id, (width, height, fps), &key, key_id, extras);
    let verb = if resumed { "/resume" } else { "/launch" };
    let xml = https_get(addr, &info, &cert, store, verb, &query, LAUNCH_WAIT)?;
    check(verb, &xml)?;
    let flag = if resumed { "resume" } else { "gamesession" };
    if tag(&xml, flag).as_deref() == Some("0") {
        return Err(format!("{verb} ditolak host"));
    }
    let session_url = tag(&xml, "sessionUrl0").ok_or("host tidak memberi alamat RTSP")?;
    Ok((info, Launched { session_url, key, key_id, resumed }))
}

/// Tutup aplikasi yang berjalan di host (Moonlight "quit").
pub fn quit(addr: &HostAddr, store: &Store) -> Result<(), String> {
    let (info, cert) = paired_host(addr, store)?;
    let xml = https_get(addr, &info, &cert, store, "/cancel", "", QUICK)?;
    check("cancel", &xml)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serverinfo_and_applist_parsing() {
        let xml = "<?xml version=\"1.0\"?><root status_code=\"200\"><hostname>CloudPC</hostname><appversion>7.1.431.-1</appversion><uniqueid>0F1E2D3C</uniqueid><HttpsPort>47984</HttpsPort><ServerCodecModeSupport>65793</ServerCodecModeSupport><currentgame>0</currentgame></root>";
        let info = parse_serverinfo(xml).unwrap();
        assert_eq!((info.hostname.as_str(), info.uniqueid.as_str(), info.https_port, info.codecs), ("CloudPC", "0F1E2D3C", 47984, 65793));
        assert!(parse_serverinfo("<root status_code=\"200\"><HttpsPort>1</HttpsPort></root>").is_err(), "tanpa uniqueid");
        assert!(parse_serverinfo("<root status_code=\"503\" status_message=\"sibuk\"></root>").unwrap_err().contains("sibuk"));

        let apps = parse_apps("<root status_code=\"200\"><App><IsHdrSupported>0</IsHdrSupported><AppTitle>Desktop</AppTitle><ID>1</ID></App><App><AppTitle>Steam Big Picture</AppTitle><ID>2</ID></App></root>");
        assert_eq!(apps, [App { id: 1, title: "Desktop".into() }, App { id: 2, title: "Steam Big Picture".into() }]);
    }

    #[test]
    fn addresses_keep_the_given_port() {
        assert_eq!(HostAddr::parse("203.0.113.10").unwrap(), HostAddr { host: "203.0.113.10".into(), http_port: 47989 });
        assert_eq!(HostAddr::parse("127.0.0.1:48100").unwrap().http_port, 48100);
    }

    #[test]
    fn launch_query_carries_moonlight_options() {
        let plain = launch_query(881, (1920, 1080, 60), &[0xAB; 16], 0xFFFF_FFFE, LaunchExtras::default());
        assert!(plain.starts_with("appid=881&mode=1920x1080x60&additionalStates=1&sops=0&rikey=abababababababababababababababab&rikeyid=-2&localAudioPlayMode=0&"), "{plain}");
        assert!(plain.ends_with("&continuousAudio=1&corever=1"));
        let options = launch_query(881, (1280, 720, 120), &[0; 16], 7, LaunchExtras { game_optimization: true, audio_on_host: true });
        assert!(options.contains("&sops=1&") && options.contains("&localAudioPlayMode=1&"), "{options}");
    }
}
