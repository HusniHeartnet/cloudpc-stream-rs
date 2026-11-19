//! Pembungkus pesan terenkripsi AES-GCM: RTSP (`rtspenc://`) dan kanal kontrol. Dipakai host maupun client; arah
//! pengirim (`Origin`) ikut menentukan IV, jadi pesan satu arah tidak pernah terbaca sebagai pesan arah lain.

use std::io::{self, Read};

use cloudpc_stream_proto::control::{control_iv, decode_message, encode_message, encrypted_header, packet_type, split_encrypted, Origin};
use cloudpc_stream_proto::rtsp::{rtsp_iv, EncryptedHeader, ENCRYPTED_HEADER_SIZE};

use crate::crypto::{gcm_decrypt, gcm_encrypt};

/// Pesan terbesar yang diterima. ANNOUNCE Moonlight ±2 KB.
const MAX_MESSAGE: usize = 16 * 1024;

/// Bungkus satu pesan RTSP: header + ciphertext.
pub fn rtsp_seal(key: &[u8; 16], seq: u32, origin: Origin, plaintext: &[u8]) -> Vec<u8> {
    let (tag, ciphertext) = gcm_encrypt(key, &rtsp_iv(seq, origin), plaintext);
    let header = EncryptedHeader { length: ciphertext.len() as u32, seq, tag };
    [&header.write()[..], &ciphertext].concat()
}

/// Baca satu pesan RTSP terenkripsi yang dikirim `origin` dan kembalikan isinya.
pub fn rtsp_read<S: Read>(stream: &mut S, key: &[u8; 16], origin: Origin) -> io::Result<Vec<u8>> {
    let invalid = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_string());
    let mut header = [0u8; ENCRYPTED_HEADER_SIZE];
    stream.read_exact(&mut header)?;
    let header = EncryptedHeader::read(&header).map_err(|e| invalid(&e.to_string()))?;
    let length = header.length as usize;
    if length == 0 || length > MAX_MESSAGE {
        return Err(invalid("panjang pesan RTSP di luar batas"));
    }
    let mut ciphertext = vec![0u8; length];
    stream.read_exact(&mut ciphertext)?;
    gcm_decrypt(key, &rtsp_iv(header.seq, origin), &header.tag, &ciphertext).ok_or_else(|| invalid("pesan RTSP gagal didekripsi"))
}

/// Bungkus satu pesan kontrol: `[0x0001][panjang][seq][tag][ciphertext]`.
pub fn seal(key: &[u8; 16], seq: u32, origin: Origin, kind: u16, payload: &[u8]) -> Vec<u8> {
    let inner = encode_message(kind, payload).expect("payload kontrol buatan sendiri selalu pendek");
    let (tag, ciphertext) = gcm_encrypt(key, &control_iv(seq, origin), &inner);
    let header = encrypted_header(seq, inner.len()).expect("payload kontrol buatan sendiri selalu pendek");
    [&header[..], &tag, &ciphertext].concat()
}

/// Mengapa sebuah paket kontrol tidak menghasilkan pesan.
#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    /// Bukan amplop terenkripsi yang utuh (termasuk pesan polos): diabaikan.
    NotEncrypted,
    /// Amplop utuh tetapi tag tidak cocok: kunci salah atau paket diubah.
    BadTag,
    /// Terdekripsi, tetapi isinya bukan pesan kontrol yang sah.
    Malformed,
}

/// Buka pesan kontrol yang dikirim `origin`; mengembalikan `(tipe, payload)`.
pub fn open(key: &[u8; 16], origin: Origin, packet: &[u8]) -> Result<(u16, Vec<u8>), OpenError> {
    let envelope = split_encrypted(packet).map_err(|_| OpenError::NotEncrypted)?;
    let plaintext = gcm_decrypt(key, &control_iv(envelope.seq, origin), envelope.tag, envelope.ciphertext).ok_or(OpenError::BadTag)?;
    let (kind, payload) = decode_message(&plaintext).map_err(|_| OpenError::Malformed)?;
    // Amplop di dalam amplop tidak pernah sah (Sunshine juga menghentikan sesi di sini).
    if kind == packet_type::ENCRYPTED {
        return Err(OpenError::Malformed);
    }
    Ok((kind, payload.to_vec()))
}
