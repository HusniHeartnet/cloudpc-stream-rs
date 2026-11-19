//! Identitas TLS host atau client: kunci RSA 2048 + sertifikat self-signed yang disimpan di folder data.

use std::fs;
use std::path::Path;

use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use rsa::RsaPrivateKey;

use crate::crypto::ParsedCert;

/// Tulis lewat file sementara lalu ganti nama, supaya pembaca tidak pernah melihat file setengah jadi.
pub fn write_atomic(path: &Path, content: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content).map_err(|e| format!("gagal menulis {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("gagal mengganti {}: {e}", path.display()))
}

/// Kunci RSA 2048 dan sertifikat self-signed host. Sertifikat ini yang dikunci (pin) client setelah pairing,
/// jadi tidak boleh berganti selama masih ada perangkat terpairing.
pub struct Identity {
    pub key: RsaPrivateKey,
    pub key_pkcs8_der: Vec<u8>,
    pub cert_pem: String,
    pub cert: ParsedCert,
}

impl Identity {
    pub fn load_or_create(dir: &Path) -> Result<Identity, String> {
        Identity::load_or_create_as(dir, "host", "CloudPC Stream Host")
    }

    /// Identitas `<stem>_key.pem` + `<stem>_cert.pem` di `dir`, dibuat baru (RSA 2048, berlaku sampai 2046) bila belum ada.
    pub fn load_or_create_as(dir: &Path, stem: &str, common_name: &str) -> Result<Identity, String> {
        let (key_path, cert_path) = (dir.join(format!("{stem}_key.pem")), dir.join(format!("{stem}_cert.pem")));
        if key_path.exists() && cert_path.exists() {
            let key_pem = fs::read_to_string(&key_path).map_err(|e| format!("gagal membaca {}: {e}", key_path.display()))?;
            let cert_pem = fs::read_to_string(&cert_path).map_err(|e| format!("gagal membaca {}: {e}", cert_path.display()))?;
            let key = RsaPrivateKey::from_pkcs8_pem(&key_pem).map_err(|e| format!("kunci {stem} rusak: {e}"))?;
            return Identity::assemble(key, cert_pem);
        }
        if key_path.exists() != cert_path.exists() {
            return Err(format!("identitas {stem} di {} tidak lengkap (hanya salah satu dari kunci/sertifikat)", dir.display()));
        }

        let key = RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).map_err(|e| format!("gagal membuat kunci RSA: {e}"))?;
        let key_pem = key.to_pkcs8_pem(LineEnding::LF).map_err(|e| format!("gagal mengodekan kunci: {e}"))?;

        let key_pair = rcgen::KeyPair::from_pkcs8_pem_and_sign_algo(&key_pem, &rcgen::PKCS_RSA_SHA256)
            .map_err(|e| format!("kunci tidak diterima pembuat sertifikat: {e}"))?;
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).map_err(|e| format!("parameter sertifikat: {e}"))?;
        params.distinguished_name = rcgen::DistinguishedName::new();
        params.distinguished_name.push(rcgen::DnType::CommonName, common_name);
        params.not_before = rcgen::date_time_ymd(2026, 1, 1);
        params.not_after = rcgen::date_time_ymd(2046, 1, 1);
        let cert_pem = params.self_signed(&key_pair).map_err(|e| format!("gagal menandatangani sertifikat: {e}"))?.pem();

        write_atomic(&key_path, key_pem.as_bytes())?;
        write_atomic(&cert_path, cert_pem.as_bytes())?;
        Identity::assemble(key, cert_pem)
    }

    fn assemble(key: RsaPrivateKey, cert_pem: String) -> Result<Identity, String> {
        let key_pkcs8_der = key.to_pkcs8_der().map_err(|e| format!("gagal mengodekan kunci: {e}"))?.as_bytes().to_vec();
        let cert = ParsedCert::from_pem(cert_pem.as_bytes())?;
        Ok(Identity { key, key_pkcs8_der, cert_pem, cert })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_identities_are_separate_and_survive_reload() {
        let dir = tempfile::tempdir().unwrap();
        let host = Identity::load_or_create(dir.path()).unwrap();
        let client = Identity::load_or_create_as(dir.path(), "client", "NVIDIA GameStream Client").unwrap();
        assert_ne!(host.cert.der, client.cert.der);
        assert!(dir.path().join("client_key.pem").exists() && dir.path().join("host_cert.pem").exists());
        let again = Identity::load_or_create_as(dir.path(), "client", "lain").unwrap();
        assert_eq!(again.cert.der, client.cert.der, "identitas yang ada tidak dibuat ulang");
    }
}
