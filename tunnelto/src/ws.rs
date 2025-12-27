use std::sync::Arc;

use tokio::net::TcpStream;
use tokio_rustls::rustls::ClientConfig;
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    error::{Error as WsError, UrlError},
    handshake::client::Response,
    protocol::WebSocketConfig,
};
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

pub async fn connect_async_force_rustls<R>(
    request: R,
) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response), WsError>
where
    R: IntoClientRequest + Unpin,
{
    connect_async_force_rustls_with_config(request, None, None).await
}

pub async fn connect_async_force_rustls_with_config<R>(
    request: R,
    ws_config: Option<WebSocketConfig>,
    rustls_client_config: Option<Arc<ClientConfig>>,
) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response), WsError>
where
    R: IntoClientRequest + Unpin,
{
    let request = request.into_client_request()?;

    let domain = request
        .uri()
        .host()
        .ok_or(WsError::Url(UrlError::NoHostName))?
        .to_string();
    let port = request
        .uri()
        .port_u16()
        .or_else(|| match request.uri().scheme_str() {
            Some("wss") => Some(443),
            Some("ws") => Some(80),
            _ => None,
        })
        .ok_or(WsError::Url(UrlError::UnsupportedUrlScheme))?;

    let addr = format!("{domain}:{port}");
    let socket = TcpStream::connect(addr).await.map_err(WsError::Io)?;

    let rustls_client_config = rustls_client_config.unwrap_or_else(default_rustls_client_config);
    let connector = Connector::Rustls(rustls_client_config);

    tokio_tungstenite::client_async_tls_with_config(
        request,
        socket,
        ws_config,
        Some(connector),
    )
    .await
}

fn default_rustls_client_config() -> Arc<ClientConfig> {
    let mut config = ClientConfig::new();
    config
        .root_store
        .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
    Arc::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Duration;

    use rcgen::{BasicConstraints, Certificate as RcgenCertificate, CertificateParams, IsCa};
    use tokio::net::TcpListener;
    use tokio_rustls::rustls::{Certificate, NoClientAuth, PrivateKey, ServerConfig};
    use tokio_rustls::TlsAcceptor;

    fn generate_ca_and_server_cert() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca_cert = RcgenCertificate::from_params(ca_params).expect("generate ca cert");

        let server_params = CertificateParams::new(vec!["localhost".to_string()]);
        let server_cert =
            RcgenCertificate::from_params(server_params).expect("generate server cert");

        let ca_der = ca_cert.serialize_der().expect("serialize ca der");
        let server_cert_der = server_cert
            .serialize_der_with_signer(&ca_cert)
            .expect("serialize server der");
        let server_key_der = server_cert.serialize_private_key_der();

        (ca_der, server_cert_der, server_key_der)
    }

    async fn spawn_tls_ws_server_once(
        cert_chain_der: Vec<Vec<u8>>,
        key_der: Vec<u8>,
    ) -> (u16, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind tcp listener");
        let port = listener.local_addr().expect("local addr").port();

        let mut server_config = ServerConfig::new(NoClientAuth::new());
        let cert_chain = cert_chain_der.into_iter().map(Certificate).collect();
        server_config
            .set_single_cert(cert_chain, PrivateKey(key_der))
            .expect("set_single_cert");
        let acceptor = TlsAcceptor::from(Arc::new(server_config));

        let task = tokio::spawn(async move {
            let Ok(Ok((tcp, _addr))) =
                tokio::time::timeout(Duration::from_secs(5), listener.accept()).await
            else {
                return;
            };

            let Ok(Ok(tls)) = tokio::time::timeout(Duration::from_secs(5), acceptor.accept(tcp)).await
            else {
                return;
            };

            let _ = tokio::time::timeout(
                Duration::from_secs(5),
                tokio_tungstenite::accept_async(tls),
            )
            .await;
        });

        (port, task)
    }

    #[tokio::test]
    async fn connect_force_rustls_errors_as_io_for_untrusted_cert() {
        let (ca_der, server_cert_der, server_key_der) = generate_ca_and_server_cert();

        let (port, server_task) =
            spawn_tls_ws_server_once(vec![server_cert_der, ca_der], server_key_der).await;
        let url = format!("wss://localhost:{port}/");

        let err = match tokio::time::timeout(
            Duration::from_secs(5),
            connect_async_force_rustls(url.as_str()),
        )
        .await
        {
            Ok(Ok((_websocket, _response))) => panic!("expected untrusted cert failure"),
            Ok(Err(err)) => err,
            Err(_elapsed) => panic!("client connect timeout"),
        };

        match err {
            WsError::Io(_) => {}
            other => panic!(
                "expected WsError::Io (rustls handshake failure), got {:?}",
                other
            ),
        }

        let _ = tokio::time::timeout(Duration::from_secs(5), server_task).await;
    }

    #[tokio::test]
    async fn connect_force_rustls_succeeds_with_custom_root() {
        let (ca_der, server_cert_der, server_key_der) = generate_ca_and_server_cert();

        let (port, server_task) = spawn_tls_ws_server_once(
            vec![server_cert_der, ca_der.clone()],
            server_key_der,
        )
        .await;
        let url = format!("wss://localhost:{port}/");

        let mut client_config = ClientConfig::new();
        client_config
            .root_store
            .add(&Certificate(ca_der))
            .expect("add root cert");
        let client_config = Arc::new(client_config);

        let (websocket, _response) = tokio::time::timeout(
            Duration::from_secs(5),
            connect_async_force_rustls_with_config(url.as_str(), None, Some(client_config)),
        )
        .await
        .expect("client connect timeout")
        .expect("expected successful connect");

        match websocket.get_ref() {
            MaybeTlsStream::Rustls(_) => {}
            _ => panic!("expected MaybeTlsStream::Rustls"),
        }

        let _ = tokio::time::timeout(Duration::from_secs(5), server_task).await;
    }
}
