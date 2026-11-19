//! HTTP/1.1 seperlunya untuk GameStream: hanya `GET` dengan query string, satu permintaan per koneksi.
//! Dipakai di atas TCP polos maupun TLS (apa pun yang `Read + Write`).

use std::io::{self, Read, Write};

/// Batas ukuran baris permintaan + header. `clientcert` saat pairing ±2,5 KB heksadesimal.
const MAX_HEAD: usize = 32 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    pub path: String,
    query: Vec<(String, String)>,
}

impl Request {
    pub fn arg(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

fn bad(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.to_string())
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
        match bytes[i] {
            b'%' if i + 2 < bytes.len() && hex(bytes[i + 1]).is_some() && hex(bytes[i + 2]).is_some() => {
                out.push(hex(bytes[i + 1]).unwrap() << 4 | hex(bytes[i + 2]).unwrap());
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse_target(target: &str) -> Request {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let query = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect();
    Request { path: percent_decode(path), query }
}

/// Baca satu permintaan sampai baris kosong. Selain `GET` ditolak.
pub fn read_request<S: Read>(stream: &mut S) -> io::Result<Request> {
    let mut head = Vec::new();
    let mut chunk = [0u8; 2048];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        if head.len() > MAX_HEAD {
            return Err(bad("header permintaan terlalu besar"));
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "koneksi ditutup sebelum permintaan lengkap"));
        }
        head.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&head);
    let mut parts = head.lines().next().unwrap_or("").split(' ');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("GET"), Some(target), Some(version)) if version.starts_with("HTTP/1.") => Ok(parse_target(target)),
        _ => Err(bad("hanya GET HTTP/1.x yang didukung")),
    }
}

pub fn write_response<S: Write>(stream: &mut S, status: u16, body: &str) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Error",
    };
    let head = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_moonlight_style_request() {
        let raw = b"GET /pair?uniqueid=0123456789ABCDEF&uuid=ab-cd&devicename=roth&phrase=getservercert&salt=00ff HTTP/1.1\r\nHost: 127.0.0.1:48100\r\n\r\n";
        let req = read_request(&mut &raw[..]).unwrap();
        assert_eq!(req.path, "/pair");
        assert_eq!(req.arg("phrase"), Some("getservercert"));
        assert_eq!(req.arg("salt"), Some("00ff"));
        assert_eq!(req.arg("tidak-ada"), None);
    }

    #[test]
    fn decodes_percent_and_plus() {
        let req = parse_target("/launch?name=My%20PC+x&bad=%zz&end=%4");
        assert_eq!(req.arg("name"), Some("My PC x"));
        assert_eq!(req.arg("bad"), Some("%zz"));
        assert_eq!(req.arg("end"), Some("%4"));
    }

    #[test]
    fn rejects_non_get_truncated_and_oversized() {
        assert!(read_request(&mut &b"POST /pair HTTP/1.1\r\n\r\n"[..]).is_err());
        assert!(read_request(&mut &b"GET /serverinfo HTTP/1.1\r\n"[..]).is_err());
        let huge = vec![b'a'; MAX_HEAD + 4096];
        assert!(read_request(&mut &huge[..]).is_err());
    }

    #[test]
    fn response_has_exact_content_length() {
        let mut out = Vec::new();
        write_response(&mut out, 200, "<root/>").unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Length: 7\r\n"));
        assert!(text.ends_with("\r\n\r\n<root/>"));
    }
}
