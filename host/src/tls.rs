//! TLS untuk port HTTPS GameStream. Client WAJIB menyodorkan sertifikat; di tingkat TLS sertifikat apa pun
//! diterima asal client membuktikan memegang kunci privatnya, lalu `nvhttp` mencocokkan DER-nya PERSIS dengan
//! daftar perangkat terpairing (sertifikat Moonlight self-signed, jadi tidak ada rantai CA untuk diperiksa).
//! Rujukan: verifikasi sertifikat di `Sunshine/src/nvhttp.cpp`.

use std::sync::Arc;

use rustls::client::danger::HandshakeSignatureValid;
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, Error, ServerConfig, SignatureScheme};

use crate::state::Identity;

#[derive(Debug)]
struct AnyCertWithProof(Arc<CryptoProvider>);

impl ClientCertVerifier for AnyCertWithProof {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(&self, _end_entity: &CertificateDer<'_>, _intermediates: &[CertificateDer<'_>], _now: UnixTime) -> Result<ClientCertVerified, Error> {
        // Otorisasi dilakukan di nvhttp dengan membandingkan DER; di sini cukup bukti kepemilikan kunci (di bawah).
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

pub fn server_config(identity: &Identity) -> Result<Arc<ServerConfig>, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cert = CertificateDer::from(identity.cert.der.clone());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key_pkcs8_der.clone()));
    let config = ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("versi TLS: {e}"))?
        .with_client_cert_verifier(Arc::new(AnyCertWithProof(provider)))
        .with_single_cert(vec![cert], key)
        .map_err(|e| format!("sertifikat host tidak diterima TLS: {e}"))?;
    Ok(Arc::new(config))
}
