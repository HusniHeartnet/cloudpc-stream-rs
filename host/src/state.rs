//! Yang disimpan host di folder state: identitas (kunci RSA + sertifikat), `uniqueid`, dan perangkat yang sudah
//! dipairing. Padanan `sunshine_state.json` + `credentials/` di Sunshine.
//!
//! Folder state berisi kunci privat host: di produksi hanya SYSTEM/Administrators yang boleh membacanya.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rand::RngCore;
use serde::{Deserialize, Serialize};

use cloudpc_stream_common::identity::write_atomic;

use crate::crypto::ParsedCert;

const STATE_FILE: &str = "state.json";
const PIN_FILE: &str = "pin.json";
const STOP_FILE: &str = "stop.json";

/// PIN yang lebih tua dari ini diabaikan dan dihapus.
pub const PIN_MAX_AGE_SECS: u64 = 120;

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}


pub use cloudpc_stream_common::identity::Identity;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedClient {
    pub name: String,
    pub cert_pem: String,
    pub paired_at: u64,
    #[serde(skip)]
    cert_der: Vec<u8>,
}

#[derive(Default, Serialize, Deserialize)]
struct StateFile {
    uniqueid: String,
    #[serde(default)]
    clients: Vec<PairedClient>,
}

/// `uniqueid` host dan daftar perangkat terpairing, disimpan di `state.json`.
pub struct Store {
    path: PathBuf,
    file: StateFile,
    /// Waktu ubah + ukuran `state.json` saat terakhir dibaca/ditulis, untuk mengenali perubahan dari proses lain.
    stamp: Option<(SystemTime, u64)>,
}

fn stamp_of(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

fn read_state(path: &Path) -> Result<StateFile, String> {
    let mut file: StateFile = match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{} rusak: {e}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => StateFile::default(),
        Err(e) => return Err(format!("gagal membaca {}: {e}", path.display())),
    };
    for client in &mut file.clients {
        client.cert_der = ParsedCert::from_pem(client.cert_pem.as_bytes())?.der;
    }
    Ok(file)
}

/// `uniqueid` yang boleh dipasang dari luar: huruf, angka, dan tanda hubung, paling panjang 64.
pub fn valid_uniqueid(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

impl Store {
    pub fn load(dir: &Path) -> Result<Store, String> {
        let path = dir.join(STATE_FILE);
        let file = read_state(&path)?;
        let mut store = Store { stamp: stamp_of(&path), path, file };
        if store.file.uniqueid.is_empty() {
            store.file.uniqueid = new_uniqueid();
            store.save()?;
        }
        Ok(store)
    }

    /// Baca ulang bila `state.json` diubah proses lain (mis. `cloudpc-stream-host unpair` dari agent).
    /// Mengembalikan `true` bila isi di memori berganti. File yang sedang rusak diabaikan (isi lama dipertahankan).
    pub fn refresh(&mut self) -> bool {
        let now = stamp_of(&self.path);
        if now == self.stamp {
            return false;
        }
        match read_state(&self.path) {
            Ok(file) if !file.uniqueid.is_empty() => {
                self.file = file;
                self.stamp = now;
                true
            }
            _ => false,
        }
    }

    /// Pasang `uniqueid` tertentu (mis. meneruskan identitas host lama supaya client mengenalinya).
    pub fn set_uniqueid(&mut self, id: &str) -> Result<(), String> {
        if !valid_uniqueid(id) {
            return Err(format!("uniqueid tidak valid: {id}"));
        }
        if self.file.uniqueid != id {
            self.file.uniqueid = id.to_string();
            self.save()?;
        }
        Ok(())
    }

    /// Cabut perangkat yang namanya cocok; mengembalikan jumlah yang dicabut.
    pub fn remove_clients(&mut self, matches: impl Fn(&str) -> bool) -> Result<usize, String> {
        self.refresh();
        let before = self.file.clients.len();
        self.file.clients.retain(|c| !matches(&c.name));
        let removed = before - self.file.clients.len();
        if removed > 0 {
            self.save()?;
        }
        Ok(removed)
    }

    pub fn uniqueid(&self) -> &str {
        &self.file.uniqueid
    }

    pub fn clients(&self) -> &[PairedClient] {
        &self.file.clients
    }

    pub fn is_paired(&self, cert_der: &[u8]) -> bool {
        self.name_of(cert_der).is_some()
    }

    /// Nama perangkat pemilik sertifikat ini, bila terpairing.
    pub fn name_of(&self, cert_der: &[u8]) -> Option<String> {
        self.file.clients.iter().find(|c| c.cert_der == cert_der).map(|c| c.name.clone())
    }

    /// Sertifikat yang sama menggantikan catatan lamanya, supaya pairing ulang tidak menumpuk duplikat.
    pub fn add_client(&mut self, name: &str, cert_pem: &str, cert_der: &[u8]) -> Result<(), String> {
        // Jangan menimpa perubahan proses lain (mis. pencabutan) dengan salinan lama di memori.
        self.refresh();
        self.file.clients.retain(|c| c.cert_der != cert_der);
        self.file.clients.push(PairedClient { name: name.to_string(), cert_pem: cert_pem.to_string(), paired_at: unix_now(), cert_der: cert_der.to_vec() });
        self.save()
    }

    fn save(&mut self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(&self.file).map_err(|e| format!("gagal mengodekan state: {e}"))?;
        write_atomic(&self.path, text.as_bytes())?;
        self.stamp = stamp_of(&self.path);
        Ok(())
    }
}

/// Bentuk UUID huruf besar seperti `uniqueid` Sunshine.
fn new_uniqueid() -> String {
    let mut b = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut b);
    let hex = crate::crypto::hex_encode(&b).to_uppercase();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

/// PIN yang dititipkan operator/agent untuk SATU percobaan pairing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingPin {
    pub pin: String,
    /// Nama perangkat yang dicatat bila pairing berhasil (agent memakai awalan `cloudpc-`).
    pub name: String,
    pub created: u64,
}

pub fn write_pin(dir: &Path, pin: &str, name: &str) -> Result<(), String> {
    if pin.len() != 4 || !pin.bytes().all(|b| b.is_ascii_digit()) {
        return Err("PIN harus 4 angka".into());
    }
    let pending = PendingPin { pin: pin.to_string(), name: name.to_string(), created: unix_now() };
    let text = serde_json::to_string(&pending).map_err(|e| format!("gagal mengodekan PIN: {e}"))?;
    write_atomic(&dir.join(PIN_FILE), text.as_bytes())
}

/// Ambil PIN yang menunggu dan HAPUS filenya (sekali pakai). PIN basi atau rusak dibuang.
pub fn take_pin(dir: &Path) -> Option<PendingPin> {
    let path = dir.join(PIN_FILE);
    let text = fs::read_to_string(&path).ok()?;
    let _ = fs::remove_file(&path);
    let pending: PendingPin = serde_json::from_str(&text).ok()?;
    (unix_now().saturating_sub(pending.created) <= PIN_MAX_AGE_SECS).then_some(pending)
}

/// Buang PIN yang belum terpakai. `true` bila memang ada.
pub fn clear_pin(dir: &Path) -> bool {
    fs::remove_file(dir.join(PIN_FILE)).is_ok()
}

/// Minta host yang sedang berjalan mengakhiri sesi stream (dibaca pemantau folder state).
pub fn request_stop(dir: &Path) -> Result<(), String> {
    write_atomic(&dir.join(STOP_FILE), b"{}")
}

/// Ada permintaan mengakhiri sesi? Permintaannya dihapus.
pub fn take_stop(dir: &Path) -> bool {
    fs::remove_file(dir.join(STOP_FILE)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_changes_are_picked_up_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let identity = Identity::load_or_create(dir.path()).unwrap();
        // "Host": memegang store di memori. "Agent": proses lain yang membuka folder yang sama.
        let mut host = Store::load(dir.path()).unwrap();
        host.add_client("cloudpc-a", &identity.cert_pem, &identity.cert.der).unwrap();
        assert!(!host.refresh(), "tulisan sendiri bukan perubahan dari luar");

        let mut agent = Store::load(dir.path()).unwrap();
        assert_eq!(agent.remove_clients(|n| n.starts_with("cloudpc-")).unwrap(), 1);
        assert!(host.refresh(), "pencabutan oleh proses lain harus terlihat");
        assert!(!host.is_paired(&identity.cert.der));
        assert_eq!(agent.remove_clients(|n| n.starts_with("cloudpc-")).unwrap(), 0);

        // Host menambah perangkat setelah agent mencabut yang lain: pencabutan itu tidak boleh hidup lagi.
        let other_dir = tempfile::tempdir().unwrap();
        let other = Identity::load_or_create(other_dir.path()).unwrap();
        let mut stale = Store::load(dir.path()).unwrap();
        stale.add_client("hp-pemilik", &identity.cert_pem, &identity.cert.der).unwrap();
        agent.refresh();
        agent.remove_clients(|n| n == "hp-pemilik").unwrap();
        host.add_client("cloudpc-b", &other.cert_pem, &other.cert.der).unwrap();
        let names: Vec<String> = Store::load(dir.path()).unwrap().clients().iter().map(|c| c.name.clone()).collect();
        assert_eq!(names, ["cloudpc-b"]);
    }

    #[test]
    fn uniqueid_can_be_pinned_and_is_validated() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load(dir.path()).unwrap();
        store.set_uniqueid("0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0").unwrap();
        assert_eq!(Store::load(dir.path()).unwrap().uniqueid(), "0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0");
        for bad in ["", "ada spasi", "<x>", &"a".repeat(65)] {
            assert!(store.set_uniqueid(bad).is_err(), "{bad:?}");
        }
        assert_eq!(store.uniqueid(), "0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0");
    }

    #[test]
    fn stop_request_and_pin_clear_are_one_shot() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!take_stop(dir.path()) && !clear_pin(dir.path()));
        request_stop(dir.path()).unwrap();
        assert!(take_stop(dir.path()) && !take_stop(dir.path()));
        write_pin(dir.path(), "1234", "x").unwrap();
        assert!(clear_pin(dir.path()));
        assert_eq!(take_pin(dir.path()), None);
    }

    #[test]
    fn pin_is_single_use_and_validated() {
        let dir = tempfile::tempdir().unwrap();
        assert!(write_pin(dir.path(), "12a4", "x").is_err());
        assert!(write_pin(dir.path(), "12345", "x").is_err());
        write_pin(dir.path(), "1234", "cloudpc-uji").unwrap();
        let pin = take_pin(dir.path()).unwrap();
        assert_eq!((pin.pin.as_str(), pin.name.as_str()), ("1234", "cloudpc-uji"));
        assert_eq!(take_pin(dir.path()), None, "PIN tidak boleh bisa dipakai dua kali");
    }

    #[test]
    fn stale_pin_is_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let old = PendingPin { pin: "1234".into(), name: "x".into(), created: unix_now() - PIN_MAX_AGE_SECS - 5 };
        fs::write(dir.path().join(PIN_FILE), serde_json::to_string(&old).unwrap()).unwrap();
        assert_eq!(take_pin(dir.path()), None);
        assert!(!dir.path().join(PIN_FILE).exists());
    }

    #[test]
    fn identity_and_uniqueid_survive_restart() {
        let dir = tempfile::tempdir().unwrap();
        let first = Identity::load_or_create(dir.path()).unwrap();
        let again = Identity::load_or_create(dir.path()).unwrap();
        assert_eq!(first.cert.der, again.cert.der);
        assert!(first.cert.verify_sha256(b"pesan", &crate::crypto::sign_sha256(&again.key, b"pesan")));
        assert!(!first.cert.verify_sha256(b"pesan lain", &crate::crypto::sign_sha256(&again.key, b"pesan")));

        let id = Store::load(dir.path()).unwrap().uniqueid().to_string();
        assert_eq!(id.len(), 36);
        assert_eq!(Store::load(dir.path()).unwrap().uniqueid(), id);
    }

    #[test]
    fn paired_clients_persist_and_same_cert_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let identity = Identity::load_or_create(dir.path()).unwrap();
        let mut store = Store::load(dir.path()).unwrap();
        assert!(!store.is_paired(&identity.cert.der));
        store.add_client("cloudpc-a", &identity.cert_pem, &identity.cert.der).unwrap();
        store.add_client("cloudpc-b", &identity.cert_pem, &identity.cert.der).unwrap();

        let reloaded = Store::load(dir.path()).unwrap();
        assert!(reloaded.is_paired(&identity.cert.der));
        assert_eq!(reloaded.clients().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["cloudpc-b"]);
    }
}
