//! HTTP/1.1 GET seperlunya untuk GameStream, di atas TCP polos atau TLS dengan sertifikat host yang DIKUNCI
//! (sertifikat dari pairing) dan sertifikat client sebagai bukti identitas.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use cloudpc_stream_common::identity::Identity;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, Error, SignatureScheme, StreamOwned};

const MAX_RESPONSE: usize = 4 * 1024 * 1024;

/// Hanya menerima sertifikat host yang persis sama dengan hasil pairing (seperti Moonlight).
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
            Err(Error::General("sertifikat host berbeda dengan saat pairing".into()))
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

/// Untuk HTTPS: identitas client dan DER sertifikat host yang dikunci.
pub struct Tls<'a> {
    pub identity: &'a Identity,
    pub host_cert_der: &'a [u8],
}

pub fn resolve(host: &str, port: u16) -> Result<SocketAddr, String> {
    (host, port).to_socket_addrs().ok().and_then(|mut a| a.next()).ok_or_else(|| format!("alamat {host} tidak dikenal"))
}

/// Pisahkan `host[:port]` (IPv6 boleh dalam kurung siku). Port bawaan dipakai bila tidak ada.
pub fn split_host_port(text: &str, default_port: u16) -> Result<(String, u16), String> {
    let bad = || format!("alamat host tidak valid: {text}");
    if let Some(rest) = text.strip_prefix('[') {
        let (host, after) = rest.split_once(']').ok_or_else(bad)?;
        return match after.strip_prefix(':') {
            Some(port) => Ok((host.to_string(), port.parse().map_err(|_| bad())?)),
            None if after.is_empty() => Ok((host.to_string(), default_port)),
            None => Err(bad()),
        };
    }
    match text.rsplit_once(':') {
        // Lebih dari satu titik dua tanpa kurung siku = IPv6 polos, tanpa port.
        Some((host, port)) if !host.contains(':') => Ok((host.to_string(), port.parse().map_err(|_| bad())?)),
        _ if !text.is_empty() => Ok((text.to_string(), default_port)),
        _ => Err(bad()),
    }
}

/// Pilah balasan HTTP mentah jadi `(status, isi)`.
pub fn parse_response(raw: &[u8]) -> Result<(u16, String), String> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n").ok_or("balasan HTTP tanpa akhir header")?;
    let status = head.split(' ').nth(1).and_then(|s| s.parse().ok()).ok_or("baris status HTTP tidak valid")?;
    // Isi dipotong sesuai Content-Length bila ada (server boleh menahan koneksi tetap terbuka).
    let length = head.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.trim().parse::<usize>().ok()));
    let body = match length {
        Some(n) if n <= body.len() => body[..n].to_string(),
        _ => body.to_string(),
    };
    Ok((status, body))
}

/// Nilai Content-Length di bagian header, bila ada.
fn content_length(head: &[u8]) -> Option<usize> {
    String::from_utf8_lossy(head).lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim().eq_ignore_ascii_case("content-length").then(|| value.trim().parse().ok()).flatten()
    })
}

/// Baca sampai koneksi ditutup atau isi selengkap Content-Length sudah ada.
fn read_response<S: Read>(stream: &mut S) -> Result<Vec<u8>, String> {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 16384];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(raw),
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            // rustls melaporkan penutupan tanpa close_notify sebagai galat; yang sudah terbaca tetap dipakai.
            Err(_) if !raw.is_empty() => return Ok(raw),
            Err(e) => return Err(format!("gagal membaca balasan host: {e}")),
        }
        if raw.len() > MAX_RESPONSE {
            return Err("balasan host terlalu besar".into());
        }
        if let Some(head_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            if content_length(&raw[..head_end]).is_some_and(|n| raw.len() - (head_end + 4) >= n) {
                return Ok(raw);
            }
        }
    }
}

/// `GET path_query` ke `host:port`. `tls` = HTTPS dengan sertifikat host yang dikunci.
pub fn get(host: &str, port: u16, path_query: &str, tls: Option<Tls>, timeout: Duration) -> Result<(u16, String), String> {
    let addr = resolve(host, port)?;
    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| format!("tidak bisa menyambung ke {host}:{port}: {e}"))?;
    let _ = tcp.set_read_timeout(Some(timeout));
    let _ = tcp.set_write_timeout(Some(Duration::from_secs(10)));
    let request = format!("GET {path_query} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
    let raw = match tls {
        None => {
            let mut tcp = tcp;
            tcp.write_all(request.as_bytes()).map_err(|e| format!("gagal mengirim ke host: {e}"))?;
            read_response(&mut tcp)?
        }
        Some(tls) => {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = ClientConfig::builder_with_provider(provider.clone())
                .with_safe_default_protocol_versions()
                .map_err(|e| format!("versi TLS: {e}"))?
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(PinnedHost { der: tls.host_cert_der.to_vec(), provider }))
                .with_client_auth_cert(vec![CertificateDer::from(tls.identity.cert.der.clone())], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(tls.identity.key_pkcs8_der.clone())))
                .map_err(|e| format!("sertifikat client tidak diterima TLS: {e}"))?;
            let name = ServerName::try_from(host.to_string()).unwrap_or_else(|_| ServerName::try_from("localhost").expect("nama tetap sah"));
            let conn = ClientConnection::new(Arc::new(config), name).map_err(|e| format!("TLS: {e}"))?;
            let mut stream = StreamOwned::new(conn, tcp);
            stream.write_all(request.as_bytes()).map_err(|e| format!("gagal mengirim ke host lewat TLS: {e}"))?;
            read_response(&mut stream)?
        }
    };
    parse_response(&raw)
}

/// Isi elemen `<name>...</name>` pertama.
pub fn tag(xml: &str, name: &str) -> Option<String> {
    let (open, close) = (format!("<{name}>"), format!("</{name}>"));
    let start = xml.find(&open)? + open.len();
    Some(xml[start..start + xml[start..].find(&close)?].trim().to_string())
}

/// Atribut `status_code` dan `status_message` di elemen `<root>`.
pub fn status(xml: &str) -> (u16, String) {
    let attribute = |name: &str| xml.split(&format!("{name}=\"")).nth(1).and_then(|rest| rest.split('"').next()).map(str::to_string);
    (attribute("status_code").and_then(|s| s.parse().ok()).unwrap_or(0), attribute("status_message").unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_and_port_parsing() {
        assert_eq!(split_host_port("203.0.113.10", 47989).unwrap(), ("203.0.113.10".into(), 47989));
        assert_eq!(split_host_port("127.0.0.1:48100", 47989).unwrap(), ("127.0.0.1".into(), 48100));
        assert_eq!(split_host_port("pc.local", 47989).unwrap(), ("pc.local".into(), 47989));
        assert_eq!(split_host_port("[::1]:48100", 47989).unwrap(), ("::1".into(), 48100));
        assert_eq!(split_host_port("[::1]", 47989).unwrap(), ("::1".into(), 47989));
        assert_eq!(split_host_port("fe80::1", 47989).unwrap(), ("fe80::1".into(), 47989), "IPv6 tanpa kurung = tanpa port");
        assert!(split_host_port("1.2.3.4:abc", 47989).is_err());
        assert!(split_host_port("", 47989).is_err());
    }

    #[test]
    fn responses_and_xml() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nContent-Length: 59\r\n\r\n<root status_code=\"200\"><paired>1</paired><x> a </x></root>\r\nsisa";
        let (code, body) = parse_response(raw).unwrap();
        assert_eq!(code, 200);
        assert!(body.ends_with("</root>"), "dipotong sesuai Content-Length: {body:?}");
        assert_eq!((tag(&body, "paired").as_deref(), tag(&body, "x").as_deref(), tag(&body, "y")), (Some("1"), Some("a"), None));
        assert_eq!(status("<root status_code=\"400\" status_message=\"Invalid uniqueid\">"), (400, "Invalid uniqueid".into()));
        assert_eq!(status("<root>"), (0, String::new()));
        assert!(parse_response(b"HTTP/1.1 200 OK\r\n").is_err());
    }
}
