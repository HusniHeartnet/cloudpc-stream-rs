//! Data client di disk: identitas TLS (kunci + sertifikat), `uniqueid` client, dan host yang sudah dipairing
//! (sertifikat host dikunci per `uniqueid` host). Padanan identitas + daftar komputer di Moonlight, tetapi TANPA
//! alamat yang diingat: client hanya menyambung ke alamat yang diberikan pemanggilnya.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use cloudpc_stream_common::identity::{write_atomic, Identity};
use rand::RngCore;
use serde::{Deserialize, Serialize};

const STORE_FILE: &str = "client.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownHost {
    pub name: String,
    pub cert_pem: String,
    pub paired_at: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct StoreFile {
    uniqueid: String,
    #[serde(default)]
    hosts: BTreeMap<String, KnownHost>,
}

pub struct Store {
    path: PathBuf,
    file: StoreFile,
    pub identity: Identity,
}

/// Folder data bawaan: `%APPDATA%\CloudPC\stream-client` (Windows) atau `~/.cloudpc-stream-client`.
pub fn default_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("CloudPC").join("stream-client");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(".cloudpc-stream-client")
}

impl Store {
    pub fn open(dir: &Path) -> Result<Store, String> {
        fs::create_dir_all(dir).map_err(|e| format!("gagal membuat folder data {}: {e}", dir.display()))?;
        // CN sama dengan Moonlight; host tidak memeriksanya, tetapi log host jadi mudah dibaca.
        let identity = Identity::load_or_create_as(dir, "client", "NVIDIA GameStream Client")?;
        let path = dir.join(STORE_FILE);
        let file: StoreFile = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{} rusak: {e}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => StoreFile::default(),
            Err(e) => return Err(format!("gagal membaca {}: {e}", path.display())),
        };
        let mut store = Store { path, file, identity };
        if store.file.uniqueid.len() != 16 {
            // 16 karakter heksadesimal, seperti uniqueid Moonlight.
            let mut raw = [0u8; 8];
            rand::rngs::OsRng.fill_bytes(&mut raw);
            store.file.uniqueid = cloudpc_stream_common::crypto::hex_encode(&raw).to_uppercase();
            store.save()?;
        }
        Ok(store)
    }

    pub fn uniqueid(&self) -> &str {
        &self.file.uniqueid
    }

    pub fn host(&self, uniqueid: &str) -> Option<&KnownHost> {
        self.file.hosts.get(uniqueid)
    }

    pub fn remember(&mut self, uniqueid: &str, host: KnownHost) -> Result<(), String> {
        self.file.hosts.insert(uniqueid.to_string(), host);
        self.save()
    }

    pub fn forget(&mut self, uniqueid: &str) -> Result<bool, String> {
        let removed = self.file.hosts.remove(uniqueid).is_some();
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(&self.file).map_err(|e| format!("gagal mengodekan data client: {e}"))?;
        write_atomic(&self.path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniqueid_identity_and_hosts_persist() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let id = store.uniqueid().to_string();
        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        store.remember("HOST-1", KnownHost { name: "CloudPC".into(), cert_pem: "pem".into(), paired_at: 5 }).unwrap();

        let mut again = Store::open(dir.path()).unwrap();
        assert_eq!(again.uniqueid(), id);
        assert_eq!(again.identity.cert.der, store.identity.cert.der);
        assert_eq!(again.host("HOST-1").map(|h| h.name.as_str()), Some("CloudPC"));
        assert!(again.forget("HOST-1").unwrap());
        assert!(!again.forget("HOST-1").unwrap());
        assert!(Store::open(dir.path()).unwrap().host("HOST-1").is_none());
    }
}
