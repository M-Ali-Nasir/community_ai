//! QUIC TLS: ephemeral self-signed certs for confidentiality.
//! Identity is Ed25519 at the application layer (ADR-0008).

use std::sync::Arc;

use community_core::{CommunityError, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn mesh_transport() -> Arc<quinn::TransportConfig> {
    let mut transport = quinn::TransportConfig::default();
    transport.max_idle_timeout(Some(
        std::time::Duration::from_secs(10)
            .try_into()
            .expect("idle timeout"),
    ));
    transport.keep_alive_interval(Some(std::time::Duration::from_secs(2)));
    Arc::new(transport)
}

pub fn make_server_config() -> Result<quinn::ServerConfig> {
    install_crypto_provider();
    let certified = rcgen::generate_simple_self_signed(vec!["peer.community-ai".into()])
        .map_err(|e| CommunityError::Network(format!("rcgen: {e}")))?;
    let cert_der = CertificateDer::from(certified.cert.der().to_vec());
    let key_der =
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der()));

    let mut server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .map_err(|e| CommunityError::Network(format!("tls server cert: {e}")))?;
    server_crypto.alpn_protocols = vec![community_protocol::PROTOCOL_ALPN.to_vec()];

    let quic_server = quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)
        .map_err(|e| CommunityError::Network(format!("quic server crypto: {e}")))?;
    let mut server = quinn::ServerConfig::with_crypto(Arc::new(quic_server));
    server.transport_config(mesh_transport());
    Ok(server)
}

pub fn make_client_config() -> Result<quinn::ClientConfig> {
    install_crypto_provider();
    let mut tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();
    tls.alpn_protocols = vec![community_protocol::PROTOCOL_ALPN.to_vec()];

    let quic_client = quinn::crypto::rustls::QuicClientConfig::try_from(tls)
        .map_err(|e| CommunityError::Network(format!("quic client crypto: {e}")))?;
    let mut client = quinn::ClientConfig::new(Arc::new(quic_client));
    client.transport_config(mesh_transport());
    Ok(client)
}

#[derive(Debug)]
struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self(Arc::new(rustls::crypto::ring::default_provider())))
    }
}

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
