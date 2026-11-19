//! Pairing dari ujung ke ujung lewat soket sungguhan: tes ini memerankan Moonlight (urutan dan rumus dari
//! `moonlight-qt/app/backend/nvpairingmanager.cpp`) melawan host yang berjalan di 127.0.0.1.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cloudpc_stream_host::control::{self, OpenError, TERMINATION_CLOSED};
use cloudpc_stream_host::input::{Action, PadState};
use cloudpc_stream_host::crypto::{ecb_decrypt, ecb_encrypt, hex_decode, hex_encode, sha256, sign_sha256, ParsedCert};
use cloudpc_stream_host::nvhttp::{self, Host, HostConfig};
use cloudpc_stream_host::rtsp;
use cloudpc_stream_host::state::{request_stop, write_pin, Identity, Store};
use cloudpc_stream_proto::control::{packet_type, Origin};
use cloudpc_stream_proto::input as wire_input;
use cloudpc_stream_proto::pairing::derive_aes_key;
use cloudpc_stream_proto::rtsp::{sdp_attributes, Request, Response};
use rusty_enet as enet;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, Error, SignatureScheme, StreamOwned};

static NEXT_BASE: AtomicU16 = AtomicU16::new(0);

struct Rig {
    host: Arc<Host>,
    state: tempfile::TempDir,
    /// Identitas "Moonlight" tiruan.
    client: Identity,
    _client_dir: tempfile::TempDir,
}

fn rig(pin_wait: Duration) -> Rig {
    let state = tempfile::tempdir().unwrap();
    let client_dir = tempfile::tempdir().unwrap();
    let client = Identity::load_or_create(client_dir.path()).unwrap();
    // Tes berjalan paralel: tiap rig mencoba port dasar berikutnya sampai ada yang bebas.
    let start = 21000 + (std::process::id() % 2000) as u16 * 10;
    for _ in 0..50 {
        let base = start + NEXT_BASE.fetch_add(30, Ordering::SeqCst);
        let cfg = HostConfig { bind: "127.0.0.1".parse().unwrap(), base_port: base, name: "PC Uji <1>".into(), state_dir: state.path().to_path_buf(), uniqueid: None, pin_wait, verbose: false, test_drop_video_every: 0, test_drop_video_burst: 1, test_silent_audio: true, test_drop_audio_every: 0, test_record_input: true, test_rumble: false };
        if let Ok(host) = nvhttp::start(cfg) {
            return Rig { host, state, client, _client_dir: client_dir };
        }
    }
    panic!("tidak menemukan port bebas untuk host uji");
}

fn split_response(raw: &[u8]) -> (u16, String) {
    let text = String::from_utf8_lossy(raw);
    let status = text.split(' ').nth(1).and_then(|s| s.parse().ok()).expect("baris status HTTP");
    (status, text.split_once("\r\n\r\n").map(|(_, body)| body.to_string()).unwrap_or_default())
}

fn request(target: &str) -> Vec<u8> {
    format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").into_bytes()
}

fn http_get(rig: &Rig, target: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", rig.host.ports.http)).unwrap();
    stream.write_all(&request(target)).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    split_response(&raw)
}

#[derive(Debug)]
struct PinnedHost {
    der: Vec<u8>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedHost {
    fn verify_server_cert(&self, end_entity: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, Error> {
        if end_entity.as_ref() == self.der {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(Error::General("sertifikat host bukan yang dikunci".into()))
        }
    }

    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// GET lewat HTTPS dengan sertifikat klien `identity`, sertifikat host dikunci seperti Moonlight setelah pairing.
fn https_get(rig: &Rig, identity: &Identity, target: &str) -> (u16, String) {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let host_der = ParsedCert::from_pem(rig.host.cert_pem().as_bytes()).unwrap().der;
    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedHost { der: host_der, provider }))
        .with_client_auth_cert(vec![CertificateDer::from(identity.cert.der.clone())], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key_pkcs8_der.clone())))
        .unwrap();
    let conn = ClientConnection::new(Arc::new(config), ServerName::try_from("localhost").unwrap()).unwrap();
    let tcp = TcpStream::connect(("127.0.0.1", rig.host.ports.https)).unwrap();
    let mut stream = StreamOwned::new(conn, tcp);
    stream.write_all(&request(target)).unwrap();
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    split_response(&raw)
}

fn tag(xml: &str, name: &str) -> Option<String> {
    let (open, close) = (format!("<{name}>"), format!("</{name}>"));
    let start = xml.find(&open)? + open.len();
    Some(xml[start..start + xml[start..].find(&close)?].to_string())
}

fn status_code(xml: &str) -> String {
    xml.split("status_code=\"").nth(1).and_then(|rest| rest.split('"').next()).unwrap_or("").to_string()
}

const UID: &str = "uniqueid=0123456789ABCDEF&uuid=test&devicename=roth&updateState=1";

/// Hasil pairing dari sudut pandang klien.
struct Outcome {
    /// Klien memverifikasi host tahu PIN (hash cocok dan tanda tangan host sah).
    host_proved_pin: bool,
    /// Jawaban host di langkah terakhir.
    paired: bool,
}

fn pair_as_moonlight(rig: &Rig, client_pin: &str) -> Outcome {
    let salt = [0x5Au8; 16];
    let key = derive_aes_key(&salt, client_pin);
    let cert_hex = hex_encode(rig.client.cert_pem.as_bytes());

    let (_, xml) = http_get(rig, &format!("/pair?{UID}&phrase=getservercert&salt={}&clientcert={cert_hex}", hex_encode(&salt)));
    assert_eq!(tag(&xml, "paired").as_deref(), Some("1"), "getservercert: {xml}");
    let host_cert = ParsedCert::from_pem(&hex_decode(&tag(&xml, "plaincert").unwrap()).unwrap()).unwrap();

    let challenge = [0x11u8; 16];
    let (_, xml) = http_get(rig, &format!("/pair?{UID}&clientchallenge={}", hex_encode(&ecb_encrypt(&key, &challenge).unwrap())));
    assert_eq!(tag(&xml, "paired").as_deref(), Some("1"), "clientchallenge: {xml}");
    let response = ecb_decrypt(&key, &hex_decode(&tag(&xml, "challengeresponse").unwrap()).unwrap()).unwrap();
    let (host_hash, host_challenge) = (&response[..32], &response[32..48]);

    let client_secret = [0x22u8; 16];
    let client_hash = sha256(&[host_challenge, &rig.client.cert.signature, &client_secret]);
    let (_, xml) = http_get(rig, &format!("/pair?{UID}&serverchallengeresp={}", hex_encode(&ecb_encrypt(&key, &client_hash).unwrap())));
    assert_eq!(tag(&xml, "paired").as_deref(), Some("1"), "serverchallengeresp: {xml}");
    let pairing_secret = hex_decode(&tag(&xml, "pairingsecret").unwrap()).unwrap();
    let (host_secret, host_signature) = pairing_secret.split_at(16);

    let host_proved_pin = host_cert.verify_sha256(host_secret, host_signature) && sha256(&[&challenge, &host_cert.signature, host_secret])[..] == *host_hash;

    let client_pairing_secret = [&client_secret[..], &sign_sha256(&rig.client.key, &client_secret)[..]].concat();
    let (_, xml) = http_get(rig, &format!("/pair?{UID}&clientpairingsecret={}", hex_encode(&client_pairing_secret)));
    Outcome { host_proved_pin, paired: tag(&xml, "paired").as_deref() == Some("1") }
}

#[test]
fn serverinfo_over_http_reports_unpaired_and_ports() {
    let rig = rig(Duration::from_millis(200));
    let (status, xml) = http_get(&rig, &format!("/serverinfo?{UID}"));
    assert_eq!((status, status_code(&xml).as_str()), (200, "200"));
    assert_eq!(tag(&xml, "hostname").as_deref(), Some("PC Uji &lt;1&gt;"));
    assert_eq!(tag(&xml, "PairStatus").as_deref(), Some("0"));
    assert_eq!(tag(&xml, "HttpsPort"), Some(rig.host.ports.https.to_string()));
    assert_eq!(tag(&xml, "uniqueid"), Some(rig.host.uniqueid()));
    assert_eq!(tag(&xml, "appversion").as_deref(), Some("7.1.431.-1"));
    assert_eq!(tag(&xml, "state").as_deref(), Some("SUNSHINE_SERVER_FREE"));
}

#[test]
fn correct_pin_pairs_and_unlocks_https() {
    let rig = rig(Duration::from_secs(5));
    let before = https_get(&rig, &rig.client, &format!("/serverinfo?{UID}"));
    assert_eq!((before.0, status_code(&before.1).as_str()), (200, "401"), "sebelum pairing HTTPS ditolak lewat XML seperti Sunshine: {}", before.1);

    write_pin(rig.state.path(), "4821", "cloudpc-uji").unwrap();
    let outcome = pair_as_moonlight(&rig, "4821");
    assert!(outcome.host_proved_pin && outcome.paired);
    assert_eq!(rig.host.paired_names(), ["cloudpc-uji"]);

    let (status, xml) = https_get(&rig, &rig.client, &format!("/pair?{UID}&phrase=pairchallenge"));
    assert_eq!((status, tag(&xml, "paired").as_deref()), (200, Some("1")));
    let (_, xml) = https_get(&rig, &rig.client, &format!("/serverinfo?{UID}"));
    assert_eq!(tag(&xml, "PairStatus").as_deref(), Some("1"));
    let (_, xml) = https_get(&rig, &rig.client, &format!("/applist?{UID}"));
    assert_eq!(tag(&xml, "AppTitle").as_deref(), Some("Desktop"));
    let (_, xml) = https_get(&rig, &rig.client, &format!("/launch?{UID}&appid=1"));
    assert_eq!(status_code(&xml), "400", "launch tanpa kunci sesi");

    // Lewat HTTP polos, endpoint khusus HTTPS tidak ada: seperti Sunshine, HTTP 200 dengan status_code 404 di XML.
    let (http, xml) = http_get(&rig, &format!("/applist?{UID}"));
    assert_eq!((http, status_code(&xml).as_str()), (200, "404"));
}

#[test]
fn wrong_pin_is_detected_by_both_sides_and_nothing_is_stored() {
    let rig = rig(Duration::from_secs(5));
    write_pin(rig.state.path(), "4821", "cloudpc-uji").unwrap();
    let outcome = pair_as_moonlight(&rig, "0000");
    assert!(!outcome.host_proved_pin, "klien harus melihat host tidak memakai PIN yang sama");
    assert!(!outcome.paired, "host tidak boleh menerima klien dengan PIN salah");
    assert!(rig.host.paired_names().is_empty());
    assert_eq!(status_code(&https_get(&rig, &rig.client, &format!("/serverinfo?{UID}")).1), "401");

    // PIN sudah terpakai: percobaan berikutnya dengan PIN benar pun harus menunggu PIN baru.
    let (_, xml) = http_get(&rig, &format!("/pair?{UID}&clientchallenge=00"));
    assert_eq!(status_code(&xml), "400", "sesi lama harus sudah dibuang: {xml}");
}

#[test]
fn pairing_without_pin_times_out() {
    let rig = rig(Duration::from_millis(300));
    let cert_hex = hex_encode(rig.client.cert_pem.as_bytes());
    let (_, xml) = http_get(&rig, &format!("/pair?{UID}&phrase=getservercert&salt={}&clientcert={cert_hex}", hex_encode(&[1u8; 16])));
    assert_eq!((status_code(&xml).as_str(), tag(&xml, "paired").as_deref()), ("408", Some("0")));
    assert_eq!(tag(&xml, "plaincert"), None, "sertifikat host tidak dibagikan tanpa PIN");
}

#[test]
fn out_of_order_and_malformed_requests_are_rejected() {
    let rig = rig(Duration::from_secs(5));
    let cert_hex = hex_encode(rig.client.cert_pem.as_bytes());

    let (_, xml) = http_get(&rig, "/pair?phrase=getservercert");
    assert_eq!(status_code(&xml), "400", "tanpa uniqueid");
    let (_, xml) = http_get(&rig, &format!("/pair?{UID}&phrase=getservercert&salt=00&clientcert={cert_hex}"));
    assert_eq!(status_code(&xml), "400", "salt pendek");
    let (_, xml) = http_get(&rig, &format!("/pair?{UID}&phrase=getservercert&salt={}&clientcert=6869", hex_encode(&[1u8; 16])));
    assert_eq!(status_code(&xml), "400", "sertifikat bukan PEM");
    let (_, xml) = http_get(&rig, &format!("/pair?{UID}&serverchallengeresp=00"));
    assert_eq!(status_code(&xml), "400", "tanpa sesi");

    // Lompat langsung ke langkah 3 setelah langkah 1: sesi dibuang.
    write_pin(rig.state.path(), "1234", "x").unwrap();
    let (_, xml) = http_get(&rig, &format!("/pair?{UID}&phrase=getservercert&salt={}&clientcert={cert_hex}", hex_encode(&[1u8; 16])));
    assert_eq!(tag(&xml, "paired").as_deref(), Some("1"));
    let (_, xml) = http_get(&rig, &format!("/pair?{UID}&serverchallengeresp={}", hex_encode(&[0u8; 32])));
    assert_eq!((status_code(&xml).as_str(), tag(&xml, "paired").as_deref()), ("400", Some("0")));
    let (_, xml) = http_get(&rig, &format!("/pair?{UID}&clientchallenge={}", hex_encode(&[0u8; 16])));
    assert_eq!(status_code(&xml), "400", "sesi sudah tidak ada");
    assert!(rig.host.paired_names().is_empty());

    // Alamat tak dikenal (Moonlight memanggil `/unpair` saat pairing gagal, `/appasset` untuk gambar): HTTP 200 + XML 404
    // seperti Sunshine. Status HTTP 404 membuat Moonlight melempar galat jaringan.
    let (http, xml) = http_get(&rig, "/unpair?uniqueid=0123456789ABCDEF");
    assert_eq!((http, status_code(&xml).as_str()), (200, "404"));
    let (http, xml) = https_get(&rig, &rig.client, "/appasset?appid=1&AssetType=2&AssetIdx=0");
    assert_eq!((http, status_code(&xml).as_str()), (200, "401"), "perangkat belum terpairing");

    // Langkah tanpa parameter yang dikenal: 400 + paired=0 seperti `fail_pair` Sunshine, dan sesinya dibuang.
    write_pin(rig.state.path(), "1234", "x").unwrap();
    let (_, xml) = http_get(&rig, &format!("/pair?{UID}&phrase=getservercert&salt={}&clientcert={cert_hex}", hex_encode(&[1u8; 16])));
    assert_eq!(tag(&xml, "paired").as_deref(), Some("1"));
    let (http, xml) = http_get(&rig, &format!("/pair?{UID}&entah=1"));
    assert_eq!((http, status_code(&xml).as_str(), tag(&xml, "paired").as_deref()), (200, "400", Some("0")));
    let (_, xml) = http_get(&rig, &format!("/pair?{UID}&clientchallenge={}", hex_encode(&[0u8; 16])));
    assert_eq!(status_code(&xml), "400", "sesi sudah dibuang");
}

/// Semua Moonlight memakai uniqueid yang sama. Pairing yang putus setelah PIN terpakai (mis. jaringan putus) tidak boleh
/// menahan uniqueid itu: percobaan ulang langsung bisa pairing, bukan ditolak 409 selama beberapa menit.
#[test]
fn retry_after_pairing_abandoned_midway_is_not_blocked() {
    let rig = rig(Duration::from_secs(5));
    let cert_hex = hex_encode(rig.client.cert_pem.as_bytes());
    write_pin(rig.state.path(), "1111", "cloudpc-putus").unwrap();
    let (_, xml) = http_get(&rig, &format!("/pair?{UID}&phrase=getservercert&salt={}&clientcert={cert_hex}", hex_encode(&[3u8; 16])));
    assert_eq!(tag(&xml, "paired").as_deref(), Some("1"));
    // ... client hilang di sini. Percobaan baru dengan PIN baru:
    write_pin(rig.state.path(), "4821", "cloudpc-uji").unwrap();
    let outcome = pair_as_moonlight(&rig, "4821");
    assert!(outcome.host_proved_pin && outcome.paired);
    assert_eq!(rig.host.paired_names(), vec!["cloudpc-uji".to_string()]);
}

/// Pairing yang masih menunggu PIN tetap menolak percobaan kedua (seperti Sunshine), supaya PIN tidak direbut.
#[test]
fn second_pairing_while_first_waits_for_pin_is_refused() {
    let rig = std::sync::Arc::new(rig(Duration::from_secs(2)));
    let cert_hex = hex_encode(rig.client.cert_pem.as_bytes());
    let first = {
        let (rig, cert_hex) = (rig.clone(), cert_hex.clone());
        std::thread::spawn(move || http_get(&rig, &format!("/pair?{UID}&phrase=getservercert&salt={}&clientcert={cert_hex}", hex_encode(&[3u8; 16]))))
    };
    std::thread::sleep(Duration::from_millis(300));
    let (_, xml) = http_get(&rig, &format!("/pair?{UID}&phrase=getservercert&salt={}&clientcert={cert_hex}", hex_encode(&[4u8; 16])));
    assert_eq!((status_code(&xml).as_str(), tag(&xml, "paired").as_deref()), ("409", Some("0")));
    let (_, xml) = first.join().unwrap();
    assert_eq!(status_code(&xml), "408", "tanpa PIN, percobaan pertama kedaluwarsa");
}

#[test]
fn stranger_certificate_stays_locked_out_after_someone_else_pairs() {
    let rig = rig(Duration::from_secs(5));
    write_pin(rig.state.path(), "4821", "cloudpc-uji").unwrap();
    assert!(pair_as_moonlight(&rig, "4821").paired);

    let other_dir = tempfile::tempdir().unwrap();
    let stranger = Identity::load_or_create(other_dir.path()).unwrap();
    let (status, xml) = https_get(&rig, &stranger, &format!("/applist?{UID}"));
    assert_eq!((status, status_code(&xml).as_str()), (200, "401"));
    assert_eq!(tag(&xml, "AppTitle"), None);
}

// ---------------------------------------------------------------------------------------------------------
// Sesi stream: /launch -> RTSP terenkripsi -> kanal kontrol ENet -> /cancel.
// ---------------------------------------------------------------------------------------------------------

const KEY: [u8; 16] = [0x42; 16];

fn paired_rig() -> Rig {
    let rig = rig(Duration::from_secs(5));
    write_pin(rig.state.path(), "4821", "cloudpc-uji").unwrap();
    assert!(pair_as_moonlight(&rig, "4821").paired);
    rig
}

fn launch(rig: &Rig) -> String {
    let (_, xml) = https_get(rig, &rig.client, &format!("/launch?{UID}&appid=1&mode=1920x1080x60&rikey={}&rikeyid=-77&corever=1&localAudioPlayMode=0", hex_encode(&KEY)));
    assert_eq!((status_code(&xml).as_str(), tag(&xml, "gamesession").as_deref()), ("200", Some("1")), "{xml}");
    tag(&xml, "sessionUrl0").unwrap()
}

/// Kirim satu pesan RTSP terenkripsi di koneksi baru (seperti Moonlight) dan baca balasannya.
fn rtsp_call(rig: &Rig, key: &[u8; 16], seq: u32, command: &str, target: &str, payload: &str) -> std::io::Result<Response> {
    let request = Request { command: command.into(), target: target.into(), headers: vec![("CSeq".into(), seq.to_string()), ("X-GS-ClientVersion".into(), "14".into())], payload: payload.as_bytes().to_vec() };
    let mut stream = TcpStream::connect(("127.0.0.1", rig.host.ports.rtsp))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(&rtsp::seal(key, seq, Origin::Client, &request.serialize()))?;
    let plaintext = rtsp::read_sealed(&mut stream, key, Origin::Host)?;
    Ok(Response::parse(&plaintext).expect("balasan RTSP"))
}

fn sdp(encryption: u32) -> String {
    [
        "v=0".to_string(),
        "s=NVIDIA Streaming Client".to_string(),
        "a=x-nv-video[0].clientViewportWd:1920 ".to_string(),
        "a=x-nv-video[0].clientViewportHt:1080 ".to_string(),
        "a=x-nv-video[0].maxFPS:60 ".to_string(),
        "a=x-nv-video[0].packetSize:1392 ".to_string(),
        "a=x-nv-vqos[0].bw.maximumBitrateKbps:20000 ".to_string(),
        "a=x-nv-vqos[0].bitStreamFormat:1 ".to_string(),
        "a=x-nv-audio.surround.numChannels:2 ".to_string(),
        "a=x-nv-video[0].videoEncoderSlicesPerFrame:1 ".to_string(),
        "a=x-nv-video[0].maxNumReferenceFrames:0 ".to_string(),
        "a=x-ml-general.featureFlags:3 ".to_string(),
        format!("a=x-ss-general.encryptionEnabled:{encryption} "),
    ]
    .join("\r\n")
        + "\r\n"
}

/// Jabat tangan RTSP lengkap; mengembalikan `connect_data` untuk kanal kontrol.
fn rtsp_handshake(rig: &Rig) -> u32 {
    let url = launch(rig);
    assert_eq!(url, format!("rtspenc://127.0.0.1:{}", rig.host.ports.rtsp));

    assert_eq!(rtsp_call(rig, &KEY, 1, "OPTIONS", &url, "").unwrap().status, 200);
    let describe = rtsp_call(rig, &KEY, 2, "DESCRIBE", &url, "").unwrap();
    assert_eq!(sdp_attributes(&describe.payload).get("x-ss-general.encryptionRequested").map(String::as_str), Some("7"));

    let audio = rtsp_call(rig, &KEY, 3, "SETUP", "streamid=audio/0/0", "").unwrap();
    assert_eq!(audio.header("Transport"), Some(format!("server_port={}", rig.host.ports.audio).as_str()));
    assert!(audio.header("Session").unwrap().starts_with("DEADBEEFCAFE"));
    let ping = audio.header("X-SS-Ping-Payload").unwrap().to_string();
    assert_eq!(ping.len(), 16);
    let video = rtsp_call(rig, &KEY, 4, "SETUP", "streamid=video/0/0", "").unwrap();
    assert_eq!((video.header("Transport"), video.header("X-SS-Ping-Payload")), (Some(format!("server_port={}", rig.host.ports.video).as_str()), Some(ping.as_str())));
    let control = rtsp_call(rig, &KEY, 5, "SETUP", "streamid=control/13/0", "").unwrap();
    assert_eq!(control.header("Transport"), Some(format!("server_port={}", rig.host.ports.control).as_str()));
    let connect_data: u32 = control.header("X-SS-Connect-Data").unwrap().parse().unwrap();
    assert_eq!(rtsp_call(rig, &KEY, 6, "SETUP", "streamid=lain/0/0", "").unwrap().status, 404);

    assert!(rig.host.stream_snapshot().is_none(), "sesi stream baru ada setelah ANNOUNCE");
    assert_eq!(rtsp_call(rig, &KEY, 7, "ANNOUNCE", "streamid=control/13/0", &sdp(7)).unwrap().status, 200);
    assert_eq!(rtsp_call(rig, &KEY, 8, "PLAY", "/", "").unwrap().status, 200);
    assert_eq!(rtsp_call(rig, &KEY, 9, "TEARDOWN", "/", "").unwrap().status, 404);
    connect_data
}

struct ControlClient {
    enet: enet::Host<UdpSocket>,
    peer: enet::PeerID,
    seq: u32,
}

impl ControlClient {
    fn connect(rig: &Rig, connect_data: u32) -> ControlClient {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut enet = enet::Host::new(socket, enet::HostSettings { peer_limit: 1, ..Default::default() }).unwrap();
        let address: SocketAddr = ([127, 0, 0, 1], rig.host.ports.control).into();
        // 0x30 kanal seperti Moonlight (CTRL_CHANNEL_COUNT).
        let peer = enet.connect(address, 0x30, connect_data).unwrap().id();
        ControlClient { enet, peer, seq: 0 }
    }

    /// Jalankan ENet sampai `done` mengembalikan true atau waktu habis.
    fn pump(&mut self, timeout: Duration, mut done: impl FnMut(Option<&enet::EventNoRef>) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let event = self.enet.service().ok().flatten().map(|e| e.no_ref());
            if done(event.as_ref()) {
                return true;
            }
            if event.is_none() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        false
    }

    fn wait_connected(&mut self) -> bool {
        self.pump(Duration::from_secs(3), |e| matches!(e, Some(enet::EventNoRef::Connect { .. })))
    }

    fn send(&mut self, kind: u16, payload: &[u8]) {
        let message = control::seal(&KEY, self.seq, Origin::Client, kind, payload);
        self.seq += 1;
        self.send_raw(&message);
    }

    fn send_raw(&mut self, bytes: &[u8]) {
        self.enet.peer_mut(self.peer).send(1, &enet::Packet::reliable(bytes)).unwrap();
    }
}

#[test]
fn full_session_launch_rtsp_control_and_cancel() {
    let rig = paired_rig();
    let connect_data = rtsp_handshake(&rig);

    let snapshot = rig.host.stream_snapshot().expect("ANNOUNCE membuat sesi stream");
    assert_eq!((snapshot.client_name.as_str(), snapshot.control_connected), ("cloudpc-uji", false));
    assert_eq!((snapshot.config.width, snapshot.config.height, snapshot.config.fps, snapshot.config.video_format), (1920, 1080, 60, 1));

    let mut client = ControlClient::connect(&rig, connect_data);
    assert!(client.wait_connected(), "kanal kontrol harus tersambung");
    // Urutan yang dikirim Moonlight setelah tersambung: minta IDR, START_B, lalu ping berkala.
    client.send(packet_type::REQUEST_IDR_FRAME, &[0, 0]);
    client.send(packet_type::START_B, &[0]);
    client.send(packet_type::PERIODIC_PING, &[0; 8]);
    client.send(packet_type::PERIODIC_PING, &[0; 8]);
    // Pesan polos harus dibuang tanpa mengganggu sesi.
    client.send_raw(&[0x06, 0x02, 0x01, 0x00, 0xFF]);
    let seen = client.pump(Duration::from_secs(3), |_| rig.host.stream_snapshot().is_some_and(|s| s.messages.get(&packet_type::PERIODIC_PING) == Some(&2)));
    assert!(seen, "host harus menerima dan mendekripsi pesan kontrol: {:?}", rig.host.stream_snapshot());
    let snapshot = rig.host.stream_snapshot().unwrap();
    assert!(snapshot.control_connected);
    assert_eq!(snapshot.messages.iter().map(|(k, v)| (*k, *v)).collect::<Vec<_>>(), [(0x0200, 2), (0x0302, 1), (0x0307, 1)]);

    // Setelah kanal kontrol tersambung, sesi peluncuran sudah terpakai: RTSP tidak dilayani lagi.
    assert!(rtsp_call(&rig, &KEY, 10, "OPTIONS", "/", "").is_err());

    let (_, xml) = https_get(&rig, &rig.client, &format!("/serverinfo?{UID}"));
    assert_eq!((tag(&xml, "currentgame").as_deref(), tag(&xml, "state").as_deref()), (Some("1"), Some("SUNSHINE_SERVER_BUSY")));
    let (_, xml) = https_get(&rig, &rig.client, &format!("/launch?{UID}&appid=1&rikey={}&rikeyid=1&corever=1", hex_encode(&KEY)));
    assert_eq!(status_code(&xml), "400", "aplikasi sudah berjalan: client harus memakai /resume");

    // /cancel: client diberi tahu lewat pesan terminasi terenkripsi, lalu diputus.
    let (_, xml) = https_get(&rig, &rig.client, &format!("/cancel?{UID}"));
    assert_eq!(tag(&xml, "cancel").as_deref(), Some("1"));
    let mut termination = None;
    let disconnected = client.pump(Duration::from_secs(5), |event| match event {
        Some(enet::EventNoRef::Receive { packet, .. }) => {
            termination = Some(control::open(&KEY, Origin::Host, packet.data()));
            false
        }
        Some(enet::EventNoRef::Disconnect { .. }) => true,
        _ => false,
    });
    assert_eq!(termination, Some(Ok((packet_type::TERMINATION, TERMINATION_CLOSED.to_be_bytes().to_vec()))));
    assert!(disconnected, "host harus memutus client setelah terminasi");
    assert!(rig.host.stream_snapshot().is_none());
    let (_, xml) = https_get(&rig, &rig.client, &format!("/serverinfo?{UID}"));
    assert_eq!((tag(&xml, "currentgame").as_deref(), tag(&xml, "state").as_deref()), (Some("0"), Some("SUNSHINE_SERVER_FREE")));
}

#[test]
fn resume_reuses_running_app_and_replaces_old_control_peer() {
    let rig = paired_rig();
    let (_, xml) = https_get(&rig, &rig.client, &format!("/resume?{UID}&rikey={}&rikeyid=1&corever=1", hex_encode(&KEY)));
    assert_eq!((status_code(&xml).as_str(), tag(&xml, "resume").as_deref()), ("503", Some("0")), "belum ada yang berjalan");

    let first_data = rtsp_handshake(&rig);
    let mut first = ControlClient::connect(&rig, first_data);
    assert!(first.wait_connected());

    // Client yang sama menyambung ulang (mis. ganti kualitas): /resume + RTSP baru menggantikan sesi lama.
    let (_, xml) = https_get(&rig, &rig.client, &format!("/resume?{UID}&rikey={}&rikeyid=1&corever=1", hex_encode(&KEY)));
    assert_eq!((status_code(&xml).as_str(), tag(&xml, "resume").as_deref()), ("200", Some("1")), "{xml}");
    let control = rtsp_call(&rig, &KEY, 1, "SETUP", "streamid=control/13/0", "").unwrap();
    let second_data: u32 = control.header("X-SS-Connect-Data").unwrap().parse().unwrap();
    assert_ne!(first_data, second_data);
    assert_eq!(rtsp_call(&rig, &KEY, 2, "ANNOUNCE", "streamid=control/13/0", &sdp(7)).unwrap().status, 200);

    assert!(first.pump(Duration::from_secs(5), |e| matches!(e, Some(enet::EventNoRef::Disconnect { .. }))), "peer lama harus diputus");
    let mut second = ControlClient::connect(&rig, second_data);
    assert!(second.wait_connected());
    second.send(packet_type::START_B, &[0]);
    assert!(second.pump(Duration::from_secs(3), |_| rig.host.stream_snapshot().is_some_and(|s| s.messages.contains_key(&packet_type::START_B))));
}

#[test]
fn launch_requires_key_and_encrypted_rtsp_support() {
    let rig = paired_rig();
    let (_, xml) = https_get(&rig, &rig.client, &format!("/launch?{UID}&appid=1&rikey=abcd&rikeyid=1&corever=1"));
    assert_eq!(status_code(&xml), "400", "kunci harus 16 byte");
    let (_, xml) = https_get(&rig, &rig.client, &format!("/launch?{UID}&appid=1&rikey={}&rikeyid=1", hex_encode(&KEY)));
    assert_eq!((status_code(&xml).as_str(), tag(&xml, "gamesession").as_deref()), ("403", Some("0")), "client tanpa RTSP terenkripsi");
    let (_, xml) = https_get(&rig, &rig.client, &format!("/launch?{UID}&appid=9&rikey={}&rikeyid=1&corever=1", hex_encode(&KEY)));
    assert_eq!(status_code(&xml), "404", "aplikasi tidak dikenal");
    assert_eq!(tag(&xml, "sessionUrl0"), None);
    // Tanpa sesi peluncuran, port RTSP menutup koneksi tanpa menjawab.
    assert!(rtsp_call(&rig, &KEY, 1, "OPTIONS", "/", "").is_err());
}

#[test]
fn rtsp_refuses_wrong_key_and_unencrypted_stream_config() {
    let rig = paired_rig();
    launch(&rig);
    assert!(rtsp_call(&rig, &[0x43; 16], 1, "OPTIONS", "/", "").is_err(), "kunci salah: tidak ada balasan");

    // Pesan polos (tanpa amplop terenkripsi) juga tidak dijawab.
    let mut plain = TcpStream::connect(("127.0.0.1", rig.host.ports.rtsp)).unwrap();
    plain.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    plain.write_all(b"OPTIONS rtsp://127.0.0.1 RTSP/1.0\r\nCSeq: 1\r\n\r\n").unwrap();
    let mut answer = Vec::new();
    let _ = plain.read_to_end(&mut answer);
    assert!(answer.is_empty(), "RTSP polos tidak boleh dijawab");

    assert_eq!(rtsp_call(&rig, &KEY, 2, "ANNOUNCE", "streamid=control/13/0", &sdp(1)).unwrap().status, 403, "video/audio tanpa enkripsi");
    assert_eq!(rtsp_call(&rig, &KEY, 3, "ANNOUNCE", "streamid=control/13/0", "a=x-nv-video[0].maxFPS:60\r\n").unwrap().status, 400);
    assert!(rig.host.stream_snapshot().is_none());
}

#[test]
fn control_channel_rejects_wrong_connect_data_and_tampered_messages() {
    let rig = paired_rig();
    let connect_data = rtsp_handshake(&rig);

    let mut stranger = ControlClient::connect(&rig, connect_data.wrapping_add(1));
    stranger.pump(Duration::from_millis(700), |e| matches!(e, Some(enet::EventNoRef::Disconnect { .. })));
    assert_eq!(rig.host.stream_snapshot().map(|s| s.control_connected), Some(false), "angka sambung salah tidak boleh mengklaim sesi");

    let mut client = ControlClient::connect(&rig, connect_data);
    assert!(client.wait_connected());
    client.send(packet_type::START_B, &[0]);
    assert!(client.pump(Duration::from_secs(3), |_| rig.host.stream_snapshot().is_some_and(|s| !s.messages.is_empty())));

    let mut forged = control::seal(&KEY, 99, Origin::Client, packet_type::INPUT_DATA, &[1, 2, 3, 4]);
    let last = forged.len() - 1;
    forged[last] ^= 1;
    assert_eq!(control::open(&KEY, Origin::Client, &forged), Err(OpenError::BadTag));
    client.send_raw(&forged);
    assert!(client.pump(Duration::from_secs(5), |e| matches!(e, Some(enet::EventNoRef::Disconnect { .. }))), "pesan yang diubah harus mengakhiri sesi");
    assert!(rig.host.stream_snapshot().is_none());
}

// ---------------------------------------------------------------------------------------------------------
// Input: paket terenkripsi lewat kanal kontrol -> tindakan Windows. Host uji hanya MENCATAT (test_record_input),
// jadi tes ini tidak pernah menggerakkan mouse atau mengetik di desktop.
// ---------------------------------------------------------------------------------------------------------

/// Kirim paket input lalu tunggu sampai host mencatat tindakan sebanyak `expected`.
fn recorded(client: &mut ControlClient, rig: &Rig, expected: usize) -> Vec<Action> {
    let mut actions = Vec::new();
    client.pump(Duration::from_secs(3), |_| {
        actions.extend(rig.host.take_recorded_input());
        actions.len() >= expected
    });
    actions
}

#[test]
fn input_packets_become_windows_actions_and_everything_is_released_at_the_end() {
    let rig = paired_rig();
    let connect_data = rtsp_handshake(&rig);
    let mut client = ControlClient::connect(&rig, connect_data);
    assert!(client.wait_connected());

    client.send(packet_type::INPUT_DATA, &wire_input::keyboard(true, 0x8057, 0, 0));
    client.send(packet_type::INPUT_DATA, &wire_input::mouse_move_rel(-4, 9));
    client.send(packet_type::INPUT_DATA, &wire_input::mouse_button(true, 1));
    client.send(packet_type::INPUT_DATA, &wire_input::scroll(120));
    // Stream 1920x1080 dan layar belum diketahui (tidak ada video di tes): tengah bidang acuan = tengah layar.
    client.send(packet_type::INPUT_DATA, &wire_input::mouse_move_abs(640, 360, (1281, 721)));
    client.send(packet_type::INPUT_DATA, &wire_input::gamepad(&wire_input::Gamepad { index: 0, active_mask: 1, buttons: 0x1000, left_trigger: 9, ..Default::default() }));
    // Paket input rusak dan paket yang belum ditangani tidak menghasilkan apa-apa dan tidak memutus sesi.
    client.send(packet_type::INPUT_DATA, &[0, 0, 0, 99, 3, 0, 0, 0]);
    client.send(packet_type::INPUT_DATA, &[0, 0, 0, 6, 0x0D, 0, 0, 0, 1, 0]);

    let actions = recorded(&mut client, &rig, 6);
    assert_eq!(actions, [
        Action::Key { vk: 0x57, scan: 0x11, extended: false, up: false },
        Action::MouseMove { dx: -4, dy: 9 },
        Action::MouseButton { button: 1, up: false },
        Action::Wheel { amount: 120, horizontal: false },
        Action::MouseTo { x: 32768, y: 32768 },
        Action::Pad { index: 0, state: Some(PadState { buttons: 0x1000, left_trigger: 9, ..Default::default() }) },
    ]);
    assert!(rig.host.stream_snapshot().is_some(), "paket rusak tidak boleh mengakhiri sesi");

    // Client menghilang selagi W dan tombol kiri masih ditekan: host harus melepas semuanya dan mencabut gamepad.
    let (_, xml) = https_get(&rig, &rig.client, &format!("/cancel?{UID}"));
    assert_eq!(tag(&xml, "cancel").as_deref(), Some("1"));
    let released = recorded(&mut client, &rig, 3);
    assert_eq!(released, [
        Action::Key { vk: 0x57, scan: 0x11, extended: false, up: true },
        Action::MouseButton { button: 1, up: true },
        Action::Pad { index: 0, state: None },
    ]);
}

#[test]
fn held_key_repeats_over_the_wire_until_key_up() {
    let rig = paired_rig();
    let connect_data = rtsp_handshake(&rig);
    let mut client = ControlClient::connect(&rig, connect_data);
    assert!(client.wait_connected());

    let down = Action::Key { vk: 0x08, scan: 0x0E, extended: false, up: false };
    client.send(packet_type::INPUT_DATA, &wire_input::keyboard(true, 0x8008, 0, 0));
    // Jeda awal 500 ms lalu tiap 33 ms: dalam ±900 ms harus ada beberapa ulangan.
    let mut actions = Vec::new();
    client.pump(Duration::from_millis(900), |_| {
        actions.extend(rig.host.take_recorded_input());
        false
    });
    assert!(actions.len() >= 4 && actions.iter().all(|a| *a == down), "ulangan Backspace: {actions:?}");

    client.send(packet_type::INPUT_DATA, &wire_input::keyboard(false, 0x8008, 0, 0));
    // Satu ulangan terakhir boleh mendahului pelepasan; yang ditunggu adalah pelepasannya sendiri.
    let release = Action::Key { vk: 0x08, scan: 0x0E, extended: false, up: true };
    let mut after_release = Vec::new();
    let released = client.pump(Duration::from_secs(3), |_| {
        after_release.extend(rig.host.take_recorded_input());
        after_release.contains(&release)
    });
    assert!(released && after_release.last() == Some(&release), "pelepasan harus tercatat paling akhir: {after_release:?}");
    client.pump(Duration::from_millis(700), |_| false);
    assert!(rig.host.take_recorded_input().is_empty(), "setelah dilepas tidak boleh ada ulangan");
}

// ---------------------------------------------------------------------------------------------------------
// Pengaturan dari proses lain (agent) lewat folder state selagi host berjalan.
// ---------------------------------------------------------------------------------------------------------

#[test]
fn unpairing_from_another_process_locks_the_device_out_and_ends_its_session() {
    let rig = paired_rig();
    let connect_data = rtsp_handshake(&rig);
    let mut client = ControlClient::connect(&rig, connect_data);
    assert!(client.wait_connected());
    assert_eq!(https_get(&rig, &rig.client, &format!("/serverinfo?{UID}")).0, 200);

    // Yang dilakukan `cloudpc-stream-host unpair --prefix cloudpc-`: membuka folder state yang sama dan menulisnya.
    let removed = Store::load(rig.state.path()).unwrap().remove_clients(|name| name.starts_with("cloudpc-")).unwrap();
    assert_eq!(removed, 1);

    let mut terminated = false;
    let disconnected = client.pump(Duration::from_secs(5), |event| match event {
        Some(enet::EventNoRef::Receive { packet, .. }) => {
            terminated |= matches!(control::open(&KEY, Origin::Host, packet.data()), Ok((packet_type::TERMINATION, _)));
            false
        }
        Some(enet::EventNoRef::Disconnect { .. }) => true,
        _ => false,
    });
    assert!(terminated && disconnected, "sesi perangkat yang dicabut harus diakhiri");
    assert!(rig.host.stream_snapshot().is_none() && rig.host.paired_names().is_empty());
    let revoked = https_get(&rig, &rig.client, &format!("/serverinfo?{UID}"));
    assert_eq!((revoked.0, status_code(&revoked.1).as_str()), (200, "401"), "sertifikat yang dicabut ditolak lewat XML (HTTP 200), supaya Moonlight jatuh ke \"belum terpairing\"");
    let (_, xml) = http_get(&rig, &format!("/serverinfo?{UID}"));
    assert_eq!(tag(&xml, "currentgame").as_deref(), Some("0"));
}

#[test]
fn stop_request_ends_the_session_but_keeps_the_pairing() {
    let rig = paired_rig();
    let connect_data = rtsp_handshake(&rig);
    let mut client = ControlClient::connect(&rig, connect_data);
    assert!(client.wait_connected());

    request_stop(rig.state.path()).unwrap();
    assert!(client.pump(Duration::from_secs(5), |e| matches!(e, Some(enet::EventNoRef::Disconnect { .. }))));
    assert!(rig.host.stream_snapshot().is_none());
    assert_eq!(rig.host.paired_names(), ["cloudpc-uji"]);
    assert_eq!(https_get(&rig, &rig.client, &format!("/serverinfo?{UID}")).0, 200);
    // Sesi baru bisa langsung dimulai lagi.
    assert!(launch(&rig).starts_with("rtspenc://"));
}
