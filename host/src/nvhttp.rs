//! Fase H1. Server HTTP (port dasar) dan HTTPS (dasar - 5) GameStream.
//!
//! Rujukan: `Sunshine/src/nvhttp.cpp`. `/serverinfo`, `/pair` (lima langkah), `/applist`, `/launch`, `/resume`,
//! `/cancel`. `Host` di sini juga memegang keadaan sesi yang dipakai `rtsp` dan `control`.
//!
//! Beda dari Sunshine: tanpa Web UI/port admin. PIN dititipkan lewat file sekali pakai di folder state
//! (`cloudpc-stream-host pin`), yang nanti ditulis agent CloudPC.

use std::collections::HashMap;
use std::io::Write;
use std::net::{IpAddr, TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cloudpc_stream_proto::pairing::derive_aes_key;
use cloudpc_stream_proto::ports::Ports;
use rand::RngCore;
use rustls::{ServerConfig, ServerConnection, StreamOwned};

use crate::crypto::{ecb_decrypt, ecb_encrypt, hex_decode, hex_encode, sha256, sign_sha256, ParsedCert};
use crate::control::Command;
use crate::http::{read_request, write_response, Request};
use crate::session::{LaunchSession, StreamSession, StreamSnapshot};
use crate::state::{take_pin, Identity, Store};
use crate::xml::Xml;

/// Versi yang dilaporkan ke client. Komponen terakhir `-1` menandai host non-NVIDIA (seperti Sunshine);
/// mayor 7 membuat Moonlight memakai SHA-256 saat pairing dan protokol generasi 7.
const APP_VERSION: &str = "7.1.431.-1";
const GFE_VERSION: &str = "3.23.0.74";
const IO_TIMEOUT: Duration = Duration::from_secs(10);

pub struct HostConfig {
    /// Alamat yang didengarkan. Bawaan CLI 127.0.0.1 supaya tidak terbuka ke jaringan tanpa sengaja.
    pub bind: IpAddr,
    pub base_port: u16,
    /// Nama host yang tampil di client.
    pub name: String,
    pub state_dir: PathBuf,
    /// `uniqueid` yang dipasang paksa (bila tidak diisi, yang tersimpan atau acak baru).
    pub uniqueid: Option<String>,
    /// Berapa lama permintaan pairing menunggu PIN dititipkan.
    pub pin_wait: Duration,
    /// Cetak kejadian sesi ke stderr (tanpa PIN, kunci, atau isi pesan).
    pub verbose: bool,
    /// HANYA UNTUK UJI: buang tiap datagram video ke-N sebelum dikirim (0 = mati), untuk menguji FEC client.
    pub test_drop_video_every: u32,
    /// HANYA UNTUK UJI: berapa datagram berturut-turut yang dibuang tiap kali (0 atau 1 = satu).
    pub test_drop_video_burst: u32,
    /// HANYA UNTUK UJI: jangan tangkap suara, kirim hening saja. Perlu saat client berjalan di mesin yang sama
    /// dengan host, karena suara yang diputar client akan tertangkap lagi (gema berputar).
    pub test_silent_audio: bool,
    /// HANYA UNTUK UJI: buang tiap datagram audio ke-N sebelum dikirim (0 = mati), untuk menguji FEC audio client.
    pub test_drop_audio_every: u32,
    /// HANYA UNTUK UJI: jangan suntikkan input ke Windows; catat tindakannya (`Host::take_recorded_input`).
    pub test_record_input: bool,
    /// Uji: kirim satu pesan getar palsu ke client begitu paket gamepad pertama tiba (tanpa ViGEm).
    pub test_rumble: bool,
    /// Keluaran suara mikrofon pemain (`--mic`). `None` = host tidak menawarkan mikrofon ke client.
    pub mic: Option<crate::mic::MicTarget>,
}

/// Dari mana permintaan datang.
enum Origin<'a> {
    Http,
    /// HTTPS; `Some(nama)` bila sertifikat klien ada di daftar perangkat terpairing.
    Https(Option<&'a str>),
}

#[derive(PartialEq, Eq)]
enum Phase {
    /// `getservercert` diterima, masih menunggu PIN.
    WaitingPin,
    GetServerCert,
    ClientChallenge,
    ServerChallengeResp,
}

struct PairSession {
    phase: Phase,
    created: Instant,
    client_cert_pem: String,
    client_cert: ParsedCert,
    client_name: String,
    key: [u8; 16],
    server_secret: [u8; 16],
    server_challenge: [u8; 16],
    client_hash: Vec<u8>,
}

/// Hasil satu langkah pairing: lanjut, atau selesai (sesi dibuang, apa pun hasilnya).
enum Step {
    Continue(Xml),
    Done(Xml),
}

pub struct Host {
    pub(crate) cfg: HostConfig,
    pub ports: Ports,
    identity: Identity,
    store: Mutex<Store>,
    pairing: Mutex<HashMap<String, PairSession>>,
    /// ID aplikasi yang "berjalan" (0 = tidak ada). Menentukan client memanggil `/launch` atau `/resume`.
    current_app: AtomicU32,
    next_launch_id: AtomicU32,
    /// Sesi peluncuran yang menunggu RTSP + kanal kontrol (paling banyak satu).
    pub(crate) launch: Mutex<Option<LaunchSession>>,
    /// Sesi stream aktif (satu GPU = satu sesi).
    pub(crate) stream: Mutex<Option<StreamSession>>,
    /// Permintaan client untuk jalur video (IDR, invalidasi frame rujukan); dikosongkan thread video tiap frame.
    pub(crate) video_requests: Mutex<Vec<crate::video::VideoRequest>>,
    /// Ukuran layar yang sedang ditangkap (0 = belum diketahui); untuk memetakan mouse absolut.
    pub(crate) desktop_size: Mutex<(u32, u32)>,
    pub(crate) recorded_input: Mutex<Vec<crate::input::Action>>,
    control_tx: Mutex<Sender<Command>>,
    /// Paket mikrofon dari kanal kontrol ke thread mikrofon (`None` = mikrofon tidak dinyalakan).
    mic_tx: Option<Mutex<Sender<crate::mic::MicMessage>>>,
}

fn random16() -> [u8; 16] {
    let mut b = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut b);
    b
}

/// Alamat lokal seperti `net::addr_to_normalized_string` Sunshine: IPv6 yang memetakan IPv4 ditulis sebagai IPv4.
fn normalized_ip(ip: std::net::IpAddr) -> String {
    match ip {
        std::net::IpAddr::V6(v6) => v6.to_ipv4_mapped().map(|v4| v4.to_string()).unwrap_or_else(|| v6.to_string()),
        v4 => v4.to_string(),
    }
}

fn pair_failed(status: u16, message: &str) -> Xml {
    Xml::error(status, message).field("paired", 0)
}

/// Ikat port, lalu layani HTTP dan HTTPS di thread latar. Mengembalikan galat bila port sudah dipakai.
pub fn start(cfg: HostConfig) -> Result<Arc<Host>, String> {
    let ports = Ports::from_base(cfg.base_port).ok_or_else(|| format!("Port dasar {} membuat port lain keluar rentang", cfg.base_port))?;
    std::fs::create_dir_all(&cfg.state_dir).map_err(|e| format!("gagal membuat folder state {}: {e}", cfg.state_dir.display()))?;
    let identity = Identity::load_or_create(&cfg.state_dir)?;
    let mut store = Store::load(&cfg.state_dir)?;
    if let Some(id) = &cfg.uniqueid {
        store.set_uniqueid(id)?;
    }
    let tls = crate::tls::server_config(&identity)?;

    let bind = |port: u16| TcpListener::bind((cfg.bind, port)).map_err(|e| format!("gagal membuka {}:{port}: {e}", cfg.bind));
    let (http, https, rtsp) = (bind(ports.http)?, bind(ports.https)?, bind(ports.rtsp)?);
    let control = UdpSocket::bind((cfg.bind, ports.control)).map_err(|e| format!("gagal membuka UDP {}:{}: {e}", cfg.bind, ports.control))?;
    let video = UdpSocket::bind((cfg.bind, ports.video)).map_err(|e| format!("gagal membuka UDP {}:{}: {e}", cfg.bind, ports.video))?;
    let audio = UdpSocket::bind((cfg.bind, ports.audio)).map_err(|e| format!("gagal membuka UDP {}:{}: {e}", cfg.bind, ports.audio))?;
    let (control_tx, control_rx) = mpsc::channel();
    let video_caps = crate::video::capabilities();
    let (mic_tx, mic_thread) = match cfg.mic.clone() {
        Some(target) => {
            let (tx, rx) = mpsc::channel();
            (Some(Mutex::new(tx)), Some((target, rx)))
        }
        None => (None, None),
    };
    let host = Arc::new(Host {
        cfg,
        ports,
        identity,
        store: Mutex::new(store),
        pairing: Mutex::new(HashMap::new()),
        current_app: AtomicU32::new(0),
        next_launch_id: AtomicU32::new(1),
        launch: Mutex::new(None),
        stream: Mutex::new(None),
        video_requests: Mutex::new(Vec::new()),
        desktop_size: Mutex::new((0, 0)),
        recorded_input: Mutex::new(Vec::new()),
        control_tx: Mutex::new(control_tx),
        mic_tx,
    });
    host.log(|| format!("encoder: {video_caps:?}"));
    // Agent mengatur host lewat folder state (cabut perangkat, akhiri sesi); pantau perubahannya.
    let for_watch = host.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(250));
        for_watch.poll_state_dir();
    });
    let for_audio = host.clone();
    thread::spawn(move || crate::audio::serve(for_audio, audio));
    let for_video = host.clone();
    thread::spawn(move || crate::video::serve(for_video, video));
    if let Some((target, packets)) = mic_thread {
        let for_mic = host.clone();
        thread::spawn(move || crate::mic::serve(for_mic, target, packets));
    }

    let for_rtsp = host.clone();
    thread::spawn(move || crate::rtsp::serve(for_rtsp, rtsp));
    let for_control = host.clone();
    thread::spawn(move || crate::control::serve(for_control, control, control_rx));

    let for_http = host.clone();
    thread::spawn(move || {
        for stream in http.incoming().flatten() {
            let host = for_http.clone();
            thread::spawn(move || host.serve_http(stream));
        }
    });
    let for_https = host.clone();
    thread::spawn(move || {
        for stream in https.incoming().flatten() {
            let (host, tls) = (for_https.clone(), tls.clone());
            thread::spawn(move || host.serve_https(stream, tls));
        }
    });
    Ok(host)
}

impl Host {
    pub fn uniqueid(&self) -> String {
        self.store.lock().unwrap().uniqueid().to_string()
    }

    pub fn cert_pem(&self) -> &str {
        &self.identity.cert_pem
    }

    pub fn paired_names(&self) -> Vec<String> {
        self.store.lock().unwrap().clients().iter().map(|c| c.name.clone()).collect()
    }

    /// Keadaan sesi stream aktif, bila ada.
    pub fn stream_snapshot(&self) -> Option<StreamSnapshot> {
        self.stream.lock().unwrap().as_ref().map(StreamSession::snapshot)
    }

    /// Tindakan input yang tercatat sejak panggilan terakhir (hanya terisi dengan `test_record_input`).
    pub fn take_recorded_input(&self) -> Vec<crate::input::Action> {
        std::mem::take(&mut *self.recorded_input.lock().unwrap())
    }

    pub(crate) fn log(&self, message: impl FnOnce() -> String) {
        if self.cfg.verbose {
            let seconds = crate::state::unix_now() % 86_400;
            eprintln!("[host {:02}:{:02}:{:02}Z] {}", seconds / 3600, seconds / 60 % 60, seconds % 60, message());
        }
    }

    /// Host ini punya keluaran mikrofon (ditawarkan ke client di DESCRIBE).
    pub(crate) fn mic_enabled(&self) -> bool {
        self.mic_tx.is_some()
    }

    /// Teruskan paket mikrofon ke thread mikrofon; `None` = sesi berakhir. Tidak pernah menunggu.
    pub(crate) fn send_mic(&self, message: crate::mic::MicMessage) {
        if let Some(tx) = &self.mic_tx {
            let _ = tx.lock().unwrap().send(message);
        }
    }

    pub(crate) fn send_control(&self, command: Command) {
        let _ = self.control_tx.lock().unwrap().send(command);
    }

    fn serve_http(&self, mut stream: TcpStream) {
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
        let Ok(request) = read_request(&mut stream) else { return };
        let local_ip = stream.local_addr().map(|a| normalized_ip(a.ip())).unwrap_or_default();
        let (status, xml) = self.route(&request, Origin::Http, &local_ip);
        let _ = write_response(&mut stream, status, &xml.render());
    }

    fn serve_https(&self, stream: TcpStream, tls: Arc<ServerConfig>) {
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
        let local_ip = stream.local_addr().map(|a| normalized_ip(a.ip())).unwrap_or_default();
        let Ok(conn) = ServerConnection::new(tls) else { return };
        let mut stream = StreamOwned::new(conn, stream);
        // Jabat tangan TLS terjadi di dalam pembacaan pertama; gagal (mis. tanpa sertifikat klien) = koneksi ditutup.
        let Ok(request) = read_request(&mut stream) else { return };
        let client = match stream.conn.peer_certificates().and_then(|certs| certs.first()) {
            Some(cert) => self.store.lock().unwrap().name_of(cert.as_ref()),
            None => None,
        };
        let (status, xml) = self.route(&request, Origin::Https(client.as_deref()), &local_ip);
        let _ = write_response(&mut stream, status, &xml.render());
        stream.conn.send_close_notify();
        let _ = stream.flush();
    }

    fn route(&self, request: &Request, origin: Origin, local_ip: &str) -> (u16, Xml) {
        let client = match origin {
            Origin::Http => None,
            Origin::Https(Some(name)) => Some(name),
            // Seperti Sunshine (`on_verify_failed`): HTTP 200 dengan `status_code="401"` di XML. Moonlight hanya jatuh
            // ke HTTP polos ("belum terpairing") bila 401 ada di XML; status HTTP 401 dianggap galat jaringan, jadi
            // perangkat yang pairing-nya sudah dicabut gagal "Failed to connect" di sesi berikutnya.
            Origin::Https(None) => return (200, Xml::error(401, "The client is not authorized. Certificate verification failed.")),
        };
        let xml = match (request.path.as_str(), client) {
            ("/serverinfo", client) => self.serverinfo(request, client.is_some(), local_ip),
            ("/pair", _) => self.pair(request),
            ("/applist", Some(_)) => Xml::ok().child("App", &[("IsHdrSupported", "0".into()), ("AppTitle", "Desktop".into()), ("ID", "1".into())]),
            ("/launch", Some(client)) => self.launch(request, client, false, local_ip),
            ("/resume", Some(client)) => self.launch(request, client, true, local_ip),
            ("/cancel", Some(client)) => self.cancel(client),
            // Seperti Sunshine (`not_found`): HTTP 200 dengan `status_code="404"` di XML. Moonlight memanggil `/unpair` saat
            // pairing gagal dan `/appasset` untuk gambar aplikasi; status HTTP selain 200 membuatnya melempar galat jaringan.
            _ => Xml::error(404, "Not Found"),
        };
        (200, xml)
    }

    /// `/launch` dan `/resume`: catat kunci sesi dari client, lalu arahkan ke RTSP terenkripsi.
    fn launch(&self, request: &Request, client: &str, resume: bool, local_ip: &str) -> Xml {
        let field = if resume { "resume" } else { "gamesession" };
        let refuse = |status: u16, message: &str| Xml::error(status, message).field(field, 0);

        let key: Option<[u8; 16]> = request.arg("rikey").and_then(hex_decode).and_then(|b| b.try_into().ok());
        let key_id = request.arg("rikeyid").and_then(|v| v.parse::<i64>().ok());
        let (Some(key), Some(key_id)) = (key, key_id) else { return refuse(400, "Missing a required launch parameter") };
        // `corever >= 1` = client mendukung RTSP terenkripsi. Host ini tidak punya mode tanpa enkripsi.
        if request.arg("corever").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0) < 1 {
            return refuse(403, "Encryption is mandatory for this host but unsupported by the client");
        }
        let running = self.current_app.load(Ordering::SeqCst);
        if resume && running == 0 {
            return refuse(503, "No running app to resume");
        }
        if !resume {
            if running != 0 {
                return refuse(400, "An app is already running on this host");
            }
            if request.arg("appid") != Some("1") {
                return refuse(404, "Cannot find requested application");
            }
        }

        let mut ping = [0u8; 8];
        rand::rngs::OsRng.fill_bytes(&mut ping);
        let session = LaunchSession {
            id: self.next_launch_id.fetch_add(1, Ordering::SeqCst),
            key,
            key_id: key_id as u32,
            client_name: client.to_string(),
            ping_payload: hex_encode(&ping).to_uppercase(),
            connect_data: rand::rngs::OsRng.next_u32(),
            rtsp_seq: 0,
            created: Instant::now(),
        };
        self.log(|| format!("{field} dari \"{client}\": sesi peluncuran #{}", session.id));
        *self.launch.lock().unwrap() = Some(session);
        self.current_app.store(1, Ordering::SeqCst);

        let address = if local_ip.contains(':') { format!("[{local_ip}]") } else { local_ip.to_string() };
        Xml::ok().field("sessionUrl0", format!("rtspenc://{address}:{}", self.ports.rtsp)).field(field, 1)
    }

    /// `/cancel`: tutup aplikasi; sesi stream yang berjalan diberi tahu lewat kanal kontrol lalu dibuang.
    fn cancel(&self, client: &str) -> Xml {
        self.log(|| format!("cancel dari \"{client}\""));
        self.end_session();
        Xml::ok().field("cancel", 1)
    }

    /// Tutup aplikasi dan akhiri sesi stream yang berjalan (client diberi tahu lewat kanal kontrol).
    fn end_session(&self) {
        self.current_app.store(0, Ordering::SeqCst);
        *self.launch.lock().unwrap() = None;
        self.send_control(Command::Terminate(None));
    }

    /// Tanggapi perubahan di folder state oleh proses lain: perangkat dicabut atau permintaan mengakhiri sesi.
    fn poll_state_dir(&self) {
        if self.store.lock().unwrap().refresh() {
            let paired = self.paired_names();
            let launching = self.launch.lock().unwrap().as_ref().map(|l| l.client_name.clone());
            let streaming = self.stream.lock().unwrap().as_ref().map(|s| s.client_name.clone());
            // Perangkat yang dicabut tidak boleh terus streaming dengan sesi yang sudah telanjur dibuka.
            if launching.iter().chain(streaming.iter()).any(|owner| !paired.contains(owner)) {
                self.log(|| "perangkat pemilik sesi dicabut: sesi diakhiri".into());
                self.end_session();
            }
        }
        if crate::state::take_stop(&self.cfg.state_dir) {
            self.log(|| "permintaan mengakhiri sesi dari folder state".into());
            self.end_session();
        }
    }

    fn serverinfo(&self, request: &Request, https: bool, local_ip: &str) -> Xml {
        // Lewat HTTPS permintaan sudah pasti dari perangkat terpairing (lihat `route`).
        let pair_status = u8::from(https && request.arg("uniqueid").is_some());
        let running = self.current_app.load(Ordering::SeqCst);
        let codecs = crate::video::capabilities().server_codec_mode();
        Xml::ok()
            .field("hostname", &self.cfg.name)
            .field("appversion", APP_VERSION)
            .field("GfeVersion", GFE_VERSION)
            .field("uniqueid", self.uniqueid())
            .field("HttpsPort", self.ports.https)
            .field("ExternalPort", self.ports.http)
            .field("MaxLumaPixelsHEVC", "1869449984")
            .field("mac", "00:00:00:00:00:00")
            // Seperti Sunshine: Moonlight menganggap LocalIP selalu IPv4; alamat IPv6 di sini menimpa alamat LAN IPv4 yang
            // tersimpan, jadi lewat IPv6 dikirim 127.0.0.1 (Moonlight tahu mengabaikannya).
            .field("LocalIP", if local_ip.contains(':') { "127.0.0.1" } else { local_ip })
            .field("ServerCodecModeSupport", codecs)
            .field("PairStatus", pair_status)
            .field("currentgame", running)
            .field("state", if running > 0 { "SUNSHINE_SERVER_BUSY" } else { "SUNSHINE_SERVER_FREE" })
    }

    fn pair(&self, request: &Request) -> Xml {
        let Some(uniqueid) = request.arg("uniqueid") else { return Xml::error(400, "Missing uniqueid parameter") };
        match request.arg("phrase") {
            Some("getservercert") => return self.pair_start(uniqueid, request),
            // Langkah terakhir Moonlight (lewat HTTPS): sampai di sini berarti sertifikatnya sudah diterima.
            Some("pairchallenge") => return Xml::ok().field("paired", 1),
            _ => {}
        }

        let mut sessions = self.pairing.lock().unwrap();
        let Some(session) = sessions.get_mut(uniqueid) else { return pair_failed(400, "Invalid uniqueid") };
        let step = if let Some(value) = request.arg("clientchallenge") {
            self.client_challenge(session, value)
        } else if let Some(value) = request.arg("serverchallengeresp") {
            self.server_challenge_resp(session, value)
        } else if let Some(value) = request.arg("clientpairingsecret") {
            self.client_pairing_secret(session, value)
        } else {
            // Seperti Sunshine (`fail_pair`): 400 + paired=0, dan sesi pairing dibuang.
            Err("Invalid pairing request")
        };
        match step {
            Ok(Step::Continue(xml)) => xml,
            Ok(Step::Done(xml)) => {
                sessions.remove(uniqueid);
                xml
            }
            // Kegagalan apa pun membuang sesi: PIN sudah terpakai, jadi tidak ada percobaan kedua untuk PIN yang sama.
            Err(message) => {
                sessions.remove(uniqueid);
                pair_failed(400, message)
            }
        }
    }

    /// Langkah 1: simpan sertifikat klien, tunggu PIN, balas sertifikat host.
    fn pair_start(&self, uniqueid: &str, request: &Request) -> Xml {
        let salt: Option<[u8; 16]> = request.arg("salt").and_then(|s| hex_decode(s.get(..32)?)).and_then(|b| b.try_into().ok());
        let Some(salt) = salt else { return pair_failed(400, "Salt too short") };
        let client_cert_pem = request.arg("clientcert").and_then(hex_decode).and_then(|b| String::from_utf8(b).ok());
        let Some(client_cert_pem) = client_cert_pem else { return pair_failed(400, "Invalid client certificate") };
        let Ok(client_cert) = ParsedCert::from_pem(client_cert_pem.as_bytes()) else { return pair_failed(400, "Invalid client certificate") };

        let session_ttl = self.cfg.pin_wait + Duration::from_secs(60);
        {
            let mut sessions = self.pairing.lock().unwrap();
            sessions.retain(|_, s| s.created.elapsed() < session_ttl);
            // Semua Moonlight memakai uniqueid yang sama (0123456789ABCDEF). Pairing yang putus di tengah jabat tangan (mis.
            // jaringan putus setelah PIN dipakai) dulu menahan uniqueid itu sampai kedaluwarsa, jadi percobaan ulang ditolak
            // 409 selama ±3 menit. Sesi yang PIN-nya sudah terpakai tidak bisa dilanjutkan siapa pun, jadi diganti saja;
            // hanya sesi yang masih menunggu PIN (percobaan yang sedang berjalan) yang menolak dengan 409 seperti Sunshine.
            match sessions.get(uniqueid) {
                Some(old) if old.phase == Phase::WaitingPin => return pair_failed(409, "A pairing session with this uniqueid already exists"),
                Some(_) => {
                    self.log(|| "pairing lama yang terputus di tengah jalan diganti percobaan baru".into());
                    sessions.remove(uniqueid);
                }
                None => {}
            }
            sessions.insert(
                uniqueid.to_string(),
                PairSession {
                    phase: Phase::WaitingPin,
                    created: Instant::now(),
                    client_cert_pem,
                    client_cert,
                    client_name: String::new(),
                    key: [0; 16],
                    server_secret: [0; 16],
                    server_challenge: [0; 16],
                    client_hash: Vec::new(),
                },
            );
        }

        // Menunggu PIN tanpa memegang kunci apa pun, supaya permintaan lain tetap dilayani.
        let deadline = Instant::now() + self.cfg.pin_wait;
        let pending = loop {
            if let Some(pending) = take_pin(&self.cfg.state_dir) {
                break Some(pending);
            }
            if Instant::now() >= deadline {
                break None;
            }
            thread::sleep(Duration::from_millis(200));
        };

        let mut sessions = self.pairing.lock().unwrap();
        let Some(pending) = pending else {
            sessions.remove(uniqueid);
            return pair_failed(408, "Pairing session expired");
        };
        let Some(session) = sessions.get_mut(uniqueid) else { return pair_failed(408, "Pairing session expired") };
        session.key = derive_aes_key(&salt, &pending.pin);
        session.client_name = pending.name;
        session.phase = Phase::GetServerCert;
        Xml::ok().field("paired", 1).field("plaincert", hex_encode(self.identity.cert_pem.as_bytes()))
    }

    /// Langkah 2: buktikan host tahu PIN dengan meng-hash tantangan klien + tanda tangan sertifikat host + rahasia host.
    fn client_challenge(&self, session: &mut PairSession, value: &str) -> Result<Step, &'static str> {
        if session.phase != Phase::GetServerCert {
            return Err("Out of order call to clientchallenge");
        }
        session.phase = Phase::ClientChallenge;
        let challenge = hex_decode(value).and_then(|b| ecb_decrypt(&session.key, &b).ok()).ok_or("Invalid clientchallenge")?;
        session.server_secret = random16();
        session.server_challenge = random16();
        let hash = sha256(&[&challenge, &self.identity.cert.signature, &session.server_secret]);
        let plaintext = [&hash[..], &session.server_challenge[..]].concat();
        let encrypted = ecb_encrypt(&session.key, &plaintext).map_err(|_| "Invalid clientchallenge")?;
        Ok(Step::Continue(Xml::ok().field("paired", 1).field("challengeresponse", hex_encode(&encrypted))))
    }

    /// Langkah 3: simpan hash klien, kirim rahasia host + tanda tangannya.
    fn server_challenge_resp(&self, session: &mut PairSession, value: &str) -> Result<Step, &'static str> {
        if session.phase != Phase::ClientChallenge {
            return Err("Out of order call to serverchallengeresp");
        }
        session.phase = Phase::ServerChallengeResp;
        session.client_hash = hex_decode(value).and_then(|b| ecb_decrypt(&session.key, &b).ok()).ok_or("Invalid serverchallengeresp")?;
        let signature = sign_sha256(&self.identity.key, &session.server_secret);
        let secret = [&session.server_secret[..], &signature[..]].concat();
        Ok(Step::Continue(Xml::ok().field("pairingsecret", hex_encode(&secret)).field("paired", 1)))
    }

    /// Langkah 4: klien membuka rahasianya. Hash harus cocok (bukti klien tahu PIN) dan tanda tangannya harus
    /// berasal dari sertifikat yang dikirim di langkah 1. Baru setelah itu perangkat dicatat.
    fn client_pairing_secret(&self, session: &mut PairSession, value: &str) -> Result<Step, &'static str> {
        if session.phase != Phase::ServerChallengeResp {
            return Err("Out of order call to clientpairingsecret");
        }
        let secret = hex_decode(value).filter(|b| b.len() > 16).ok_or("Client pairing secret too short")?;
        let (client_secret, signature) = secret.split_at(16);
        let expected = sha256(&[&session.server_challenge, &session.client_cert.signature, client_secret]);
        let paired = expected[..] == session.client_hash[..]
            && session.client_cert.verify_sha256(client_secret, signature)
            && self.store.lock().unwrap().add_client(&session.client_name, &session.client_cert_pem, &session.client_cert.der).is_ok();
        Ok(Step::Done(Xml::ok().field("paired", u8::from(paired))))
    }
}
