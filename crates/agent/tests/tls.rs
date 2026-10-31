//! A server behind a proxy with its own CA (Caddy's `tls internal`): once the CA is trusted on
//! the PC, the agent reaches it over HTTPS. Its own test binary, because it sets
//! `SSL_CERT_FILE` for the whole process.

use std::sync::Arc;

use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
use spielstand_agent::client::Client;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        ServerConfig,
        crypto::ring,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    },
};

/// A local CA and a certificate it signed for `localhost`.
fn local_ca() -> (String, CertificateDer<'static>, PrivateKeyDer<'static>) {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_cert = ca.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca, &ca_key);
    let key = KeyPair::generate().unwrap();
    let cert = CertificateParams::new(vec!["localhost".into()])
        .unwrap()
        .signed_by(&key, &issuer)
        .unwrap();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
    (ca_cert.pem(), cert.der().clone(), key)
}

/// Answers every HTTPS request with `body`; returns its port.
async fn https_server(cert: CertificateDer<'static>, key: PrivateKeyDer<'static>, body: &'static str) -> u16 {
    let config = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let mut request = [0u8; 4096];
                let _ = tls.read(&mut request).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tls.write_all(response.as_bytes()).await;
                let _ = tls.shutdown().await;
            });
        }
    });
    port
}

#[tokio::test]
async fn a_server_with_a_local_ca_is_reached_once_the_ca_is_trusted() {
    let (ca, cert, key) = local_ca();
    let dir = tempfile::tempdir().unwrap();
    let trusted = dir.path().join("ca.pem");
    std::fs::write(&trusted, ca).unwrap();
    // What `update-ca-trust` does for the PC, for this process only.
    // SAFETY: set before any client is built, in a test binary of its own.
    unsafe { std::env::set_var("SSL_CERT_FILE", &trusted) };
    let port = https_server(cert, key, "agent").await;

    let client = Client::new(&format!("https://localhost:{port}"), "token");
    assert_eq!(client.agent_binary().await.unwrap(), b"agent");
}
