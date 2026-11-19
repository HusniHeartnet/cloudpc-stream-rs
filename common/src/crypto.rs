//! Kripto untuk pairing GameStream: AES-128-ECB tanpa padding, SHA-256, tanda tangan RSA PKCS#1 v1.5 + SHA-256,
//! dan pembacaan sertifikat X.509. Rujukan: `Sunshine/src/crypto.cpp`.

use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes128;
use aes_gcm::aead::AeadInPlace;
use aes_gcm::Aes128Gcm;
use rsa::pkcs1v15::{Signature, SigningKey, VerifyingKey};
use rsa::pkcs8::DecodePublicKey;
use rsa::sha2::{Digest, Sha256};
use rsa::signature::{SignatureEncoding, Signer, Verifier};
use rsa::{RsaPrivateKey, RsaPublicKey};
use x509_cert::der::{Decode, DecodePem, Encode};
use x509_cert::Certificate;

pub fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 0xF)] as char);
    }
    out
}

pub fn hex_decode(text: &str) -> Option<Vec<u8>> {
    let text = text.as_bytes();
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let nibble = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    text.chunks(2).map(|pair| Some(nibble(pair[0])? << 4 | nibble(pair[1])?)).collect()
}

pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn ecb(key: &[u8; 16], data: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return Err("data AES-ECB harus kelipatan 16 byte".into());
    }
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut out = data.to_vec();
    for block in out.chunks_mut(16) {
        let block = GenericArray::from_mut_slice(block);
        if encrypt {
            cipher.encrypt_block(block);
        } else {
            cipher.decrypt_block(block);
        }
    }
    Ok(out)
}

pub fn ecb_encrypt(key: &[u8; 16], data: &[u8]) -> Result<Vec<u8>, String> {
    ecb(key, data, true)
}

pub fn ecb_decrypt(key: &[u8; 16], data: &[u8]) -> Result<Vec<u8>, String> {
    ecb(key, data, false)
}

/// AES-128-GCM tanpa data tambahan; mengembalikan `(tag, ciphertext)` terpisah karena protokol menaruh tag di depan.
pub fn gcm_encrypt(key: &[u8; 16], iv: &[u8; 12], plaintext: &[u8]) -> ([u8; 16], Vec<u8>) {
    let mut buffer = plaintext.to_vec();
    let tag = Aes128Gcm::new(GenericArray::from_slice(key))
        .encrypt_in_place_detached(GenericArray::from_slice(iv), b"", &mut buffer)
        .expect("AES-GCM hanya gagal untuk pesan di atas 64 GiB");
    (tag.into(), buffer)
}

/// `None` bila tag tidak cocok (kunci salah, IV salah, atau pesan diubah di jalan).
pub fn gcm_decrypt(key: &[u8; 16], iv: &[u8; 12], tag: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    if tag.len() != 16 {
        return None;
    }
    let mut buffer = ciphertext.to_vec();
    Aes128Gcm::new(GenericArray::from_slice(key))
        .decrypt_in_place_detached(GenericArray::from_slice(iv), b"", &mut buffer, GenericArray::from_slice(tag))
        .ok()?;
    Some(buffer)
}

/// AES-128-CBC dengan padding PKCS#7 (dipakai untuk payload audio). Hasil selalu kelipatan 16 byte.
pub fn cbc_encrypt(key: &[u8; 16], iv: &[u8; 16], plaintext: &[u8]) -> Vec<u8> {
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let pad = 16 - plaintext.len() % 16;
    let mut out = plaintext.to_vec();
    out.resize(plaintext.len() + pad, pad as u8);
    let mut previous = *iv;
    for block in out.chunks_mut(16) {
        for (b, p) in block.iter_mut().zip(previous) {
            *b ^= p;
        }
        cipher.encrypt_block(GenericArray::from_mut_slice(block));
        previous.copy_from_slice(block);
    }
    out
}

/// Kebalikan `cbc_encrypt`; `None` bila panjang atau padding tidak sah.
pub fn cbc_decrypt(key: &[u8; 16], iv: &[u8; 16], ciphertext: &[u8]) -> Option<Vec<u8>> {
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(16) {
        return None;
    }
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut out = ciphertext.to_vec();
    let mut previous = *iv;
    for (block, encrypted) in out.chunks_mut(16).zip(ciphertext.chunks(16)) {
        cipher.decrypt_block(GenericArray::from_mut_slice(block));
        for (b, p) in block.iter_mut().zip(previous) {
            *b ^= p;
        }
        previous.copy_from_slice(encrypted);
    }
    let pad = usize::from(*out.last()?);
    if pad == 0 || pad > 16 || out[out.len() - pad..].iter().any(|b| usize::from(*b) != pad) {
        return None;
    }
    out.truncate(out.len() - pad);
    Some(out)
}

pub fn sign_sha256(key: &RsaPrivateKey, message: &[u8]) -> Vec<u8> {
    SigningKey::<Sha256>::new(key.clone()).sign(message).to_vec()
}

/// Sertifikat X.509 yang sudah dipilah: DER utuh, field tanda tangannya, dan kunci publik RSA.
pub struct ParsedCert {
    pub der: Vec<u8>,
    /// Isi field `signatureValue`; ikut di-hash saat pairing supaya sertifikat terikat ke PIN.
    pub signature: Vec<u8>,
    public_key: RsaPublicKey,
}

impl ParsedCert {
    pub fn from_pem(pem: &[u8]) -> Result<ParsedCert, String> {
        let cert = Certificate::from_pem(pem).map_err(|e| format!("sertifikat PEM tidak valid: {e}"))?;
        ParsedCert::from_cert(cert)
    }

    pub fn from_der(der: &[u8]) -> Result<ParsedCert, String> {
        let cert = Certificate::from_der(der).map_err(|e| format!("sertifikat DER tidak valid: {e}"))?;
        ParsedCert::from_cert(cert)
    }

    fn from_cert(cert: Certificate) -> Result<ParsedCert, String> {
        let der = cert.to_der().map_err(|e| format!("sertifikat tidak bisa dikodekan ulang: {e}"))?;
        let spki = cert.tbs_certificate.subject_public_key_info.to_der().map_err(|e| format!("kunci publik sertifikat rusak: {e}"))?;
        let public_key = RsaPublicKey::from_public_key_der(&spki).map_err(|e| format!("kunci publik sertifikat bukan RSA: {e}"))?;
        Ok(ParsedCert { der, signature: cert.signature.raw_bytes().to_vec(), public_key })
    }

    pub fn verify_sha256(&self, message: &[u8], signature: &[u8]) -> bool {
        let Ok(signature) = Signature::try_from(signature) else { return false };
        VerifyingKey::<Sha256>::new(self.public_key.clone()).verify(message, &signature).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip_and_rejects_garbage() {
        assert_eq!(hex_encode(&[0x00, 0xAB, 0xff]), "00abff");
        assert_eq!(hex_decode("00ABff"), Some(vec![0x00, 0xAB, 0xFF]));
        assert_eq!(hex_decode("abc"), None);
        assert_eq!(hex_decode("zz"), None);
    }

    #[test]
    fn aes_ecb_matches_fips_197_vector() {
        // FIPS-197 lampiran C.1.
        let key: [u8; 16] = std::array::from_fn(|i| i as u8);
        let plain = hex_decode("00112233445566778899aabbccddeeff").unwrap();
        let cipher = ecb_encrypt(&key, &plain).unwrap();
        assert_eq!(hex_encode(&cipher), "69c4e0d86a7b0430d8cdb78070b4c55a");
        assert_eq!(ecb_decrypt(&key, &cipher).unwrap(), plain);
    }

    #[test]
    fn aes_gcm_matches_nist_vector_and_detects_tampering() {
        // Kasus uji 2 dari spesifikasi GCM (McGrew & Viega): kunci, IV, dan plaintext nol.
        let (tag, cipher) = gcm_encrypt(&[0; 16], &[0; 12], &[0; 16]);
        assert_eq!(hex_encode(&cipher), "0388dace60b6a392f328c2b971b2fe78");
        assert_eq!(hex_encode(&tag), "ab6e47d42cec13bdf53a67b21257bddf");
        assert_eq!(gcm_decrypt(&[0; 16], &[0; 12], &tag, &cipher), Some(vec![0; 16]));

        let mut flipped = cipher.clone();
        flipped[3] ^= 1;
        assert_eq!(gcm_decrypt(&[0; 16], &[0; 12], &tag, &flipped), None);
        assert_eq!(gcm_decrypt(&[1; 16], &[0; 12], &tag, &cipher), None, "kunci salah");
        assert_eq!(gcm_decrypt(&[0; 16], &[1; 12], &tag, &cipher), None, "IV salah");
        assert_eq!(gcm_decrypt(&[0; 16], &[0; 12], &tag[..15], &cipher), None);
    }

    #[test]
    fn aes_cbc_matches_nist_vector_and_pads_with_pkcs7() {
        // NIST SP 800-38A F.2.1 (CBC-AES128), blok pertama.
        let key: [u8; 16] = hex_decode("2b7e151628aed2a6abf7158809cf4f3c").unwrap().try_into().unwrap();
        let iv: [u8; 16] = std::array::from_fn(|i| i as u8);
        let plain = hex_decode("6bc1bee22e409f96e93d7e117393172a").unwrap();
        let cipher = cbc_encrypt(&key, &iv, &plain);
        assert_eq!(cipher.len(), 32, "blok penuh mendapat satu blok padding");
        assert_eq!(hex_encode(&cipher[..16]), "7649abac8119b246cee98e9b12e9197d");
        assert_eq!(cbc_decrypt(&key, &iv, &cipher), Some(plain));

        for len in [0usize, 1, 15, 16, 17, 100] {
            let data: Vec<u8> = (0..len as u8).collect();
            let cipher = cbc_encrypt(&key, &iv, &data);
            assert_eq!(cipher.len(), (len / 16 + 1) * 16);
            assert_eq!(cbc_decrypt(&key, &iv, &cipher), Some(data));
        }
        assert_eq!(cbc_decrypt(&key, &iv, &cipher[..15]), None);
        assert_eq!(cbc_decrypt(&[0; 16], &iv, &cipher), None, "kunci salah merusak padding");
    }

    #[test]
    fn ecb_rejects_unaligned_input() {
        assert!(ecb_encrypt(&[0; 16], &[0; 15]).is_err());
        assert!(ecb_decrypt(&[0; 16], &[]).is_err());
    }

    #[test]
    fn sha256_of_parts_equals_sha256_of_concatenation() {
        assert_eq!(sha256(&[b"ab", b"c"]), sha256(&[b"abc"]));
        assert_eq!(hex_encode(&sha256(&[b"abc"])), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
