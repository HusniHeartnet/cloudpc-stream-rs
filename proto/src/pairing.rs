//! Pairing lewat PIN (`GET /pair`, `nvhttp.cpp` di Sunshine dan `nvpairingmanager.cpp` di moonlight-qt).
//!
//! Urutan panggilan, semuanya ke `/pair` dengan `uniqueid` yang sama:
//! 1. `phrase=getservercert` + `salt` + `clientcert` -> host membalas `plaincert` setelah PIN dimasukkan.
//! 2. `clientchallenge` (AES-128-ECB dengan kunci dari PIN) -> `challengeresponse`.
//! 3. `serverchallengeresp` -> `pairingsecret` (rahasia host + tanda tangan).
//! 4. `clientpairingsecret` -> `paired=1` bila tanda tangan dan hash cocok.
//! 5. `phrase=pairchallenge` lewat HTTPS dengan sertifikat klien -> konfirmasi.
//!
//! Di sini baru turunan kunci; langkah 2-4 (ECB, SHA-256, tanda tangan RSA) ada di fase pairing host/client.

use sha2::{Digest, Sha256};

/// Kunci AES-128 untuk tantangan pairing: 16 byte pertama dari SHA-256(salt || PIN).
pub fn derive_aes_key(salt: &[u8; 16], pin: &str) -> [u8; 16] {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(pin.as_bytes());
    let digest = hasher.finalize();
    let mut key = [0u8; 16];
    key.copy_from_slice(&digest[..16]);
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_matches_reference_sha256() {
        // python: hashlib.sha256(bytes(range(16)) + b"1234").hexdigest()[:32]
        let salt: [u8; 16] = std::array::from_fn(|i| i as u8);
        let hex: String = derive_aes_key(&salt, "1234").iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "bad0b4f7cae08eb7c1b5acc763a8ed25");
    }

    #[test]
    fn different_pin_gives_different_key() {
        let salt = [7u8; 16];
        assert_ne!(derive_aes_key(&salt, "1234"), derive_aes_key(&salt, "1235"));
    }
}
