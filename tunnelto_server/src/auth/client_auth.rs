use crate::auth::reconnect_token::ReconnectTokenPayload;
use crate::auth::{AuthResult, AuthService};
use crate::{ReconnectToken, CONFIG};
use futures::{SinkExt, StreamExt};
use tracing::error;
use tunnelto_lib::{ClientHello, ClientId, ClientType, ServerHello};
use warp::filters::ws::{Message, WebSocket};

pub struct ClientHandshake {
    pub id: ClientId,
    pub sub_domain: String,
    pub is_anonymous: bool,
}

#[tracing::instrument(skip(websocket))]
pub async fn auth_client_handshake(
    mut websocket: WebSocket,
) -> Option<(WebSocket, ClientHandshake)> {
    let client_hello_data = match websocket.next().await {
        Some(Ok(msg)) => msg,
        _ => {
            error!("no client init message");
            return None;
        }
    };

    auth_client(client_hello_data.as_bytes(), websocket).await
}

#[tracing::instrument(skip(client_hello_data, websocket))]
async fn auth_client(
    client_hello_data: &[u8],
    mut websocket: WebSocket,
) -> Option<(WebSocket, ClientHandshake)> {
    // parse the client hello
    let client_hello: ClientHello = match serde_json::from_slice(client_hello_data) {
        Ok(ch) => ch,
        Err(error) => {
            error!(?error, "invalid client hello");
            let data = serde_json::to_vec(&ServerHello::AuthFailed).unwrap_or_default();
            let _ = websocket.send(Message::binary(data)).await;
            return None;
        }
    };

    let (auth_key, client_id, requested_sub_domain) = match client_hello.client_type {
        ClientType::Anonymous => {
            let data = serde_json::to_vec(&ServerHello::AuthFailed).unwrap_or_default();
            let _ = websocket.send(Message::binary(data)).await;
            return None;

            // // determine the client and subdomain
            // let (client_id, sub_domain) =
            //     match (client_hello.reconnect_token, client_hello.sub_domain) {
            //         (Some(token), _) => {
            //             return handle_reconnect_token(token, websocket).await;
            //         }
            //         (None, Some(sd)) => (
            //             ClientId::generate(),
            //             ServerHello::prefixed_random_domain(&sd),
            //         ),
            //         (None, None) => (ClientId::generate(), ServerHello::random_domain()),
            //     };

            // return Some((
            //     websocket,
            //     ClientHandshake {
            //         id: client_id,
            //         sub_domain,
            //         is_anonymous: true,
            //     },
            // ));
        }
        ClientType::Auth { key } => match client_hello.sub_domain {
            Some(requested_sub_domain) => {
                let client_id = key.client_id();
                let (ws, sub_domain) = match sanitize_sub_domain_and_pre_validate(
                    websocket,
                    requested_sub_domain,
                    &client_id,
                )
                .await
                {
                    Some(s) => s,
                    None => return None,
                };
                websocket = ws;

                (key, client_id, sub_domain)
            }
            None => {
                if let Some(token) = client_hello.reconnect_token {
                    return handle_reconnect_token(token, websocket).await;
                } else {
                    let sub_domain = ServerHello::random_domain();
                    let client_id = key.client_id();
                    (key, client_id, sub_domain)
                }
            }
        },
    };

    tracing::info!(requested_sub_domain=%requested_sub_domain, "will auth sub domain");

    // next authenticate the sub-domain
    let sub_domain = match crate::AUTH_DB_SERVICE
        .auth_sub_domain(&auth_key.0, &requested_sub_domain)
        .await
    {
        Ok(AuthResult::Available) | Ok(AuthResult::ReservedByYou) => requested_sub_domain,
        Ok(AuthResult::ReservedByYouButDelinquent) | Ok(AuthResult::PaymentRequired) => {
            // note: delinquent payments get a random suffix
            // ServerHello::prefixed_random_domain(&requested_sub_domain)
            // TODO: create free trial domain
            tracing::info!(requested_sub_domain=%requested_sub_domain, "payment required");
            let data = serde_json::to_vec(&ServerHello::AuthFailed).unwrap_or_default();
            let _ = websocket.send(Message::binary(data)).await;
            return None;
        }
        Ok(AuthResult::ReservedByOther) => {
            let data = serde_json::to_vec(&ServerHello::SubDomainInUse).unwrap_or_default();
            let _ = websocket.send(Message::binary(data)).await;
            return None;
        }
        Err(error) => {
            error!(?error, "error auth-ing user");
            let data = serde_json::to_vec(&ServerHello::AuthFailed).unwrap_or_default();
            let _ = websocket.send(Message::binary(data)).await;
            return None;
        }
    };

    tracing::info!(subdomain=%sub_domain, "did auth sub_domain");

    Some((
        websocket,
        ClientHandshake {
            id: client_id,
            sub_domain,
            is_anonymous: false,
        },
    ))
}

#[tracing::instrument(skip(token, websocket))]
async fn handle_reconnect_token(
    token: ReconnectToken,
    mut websocket: WebSocket,
) -> Option<(WebSocket, ClientHandshake)> {
    let payload = match ReconnectTokenPayload::verify(token, &CONFIG.master_sig_key) {
        Ok(payload) => payload,
        Err(error) => {
            error!(?error, "invalid reconnect token");
            let data = serde_json::to_vec(&ServerHello::AuthFailed).unwrap_or_default();
            let _ = websocket.send(Message::binary(data)).await;
            return None;
        }
    };

    tracing::debug!(
        client_id=%&payload.client_id,
        "accepting reconnect token from client",
    );

    Some((
        websocket,
        ClientHandshake {
            id: payload.client_id,
            sub_domain: payload.sub_domain,
            is_anonymous: true,
        },
    ))
}

async fn sanitize_sub_domain_and_pre_validate(
    mut websocket: WebSocket,
    requested_sub_domain: String,
    client_id: &ClientId,
) -> Option<(WebSocket, String)> {
    // ignore uppercase
    let sub_domain = requested_sub_domain.to_lowercase();

    if sub_domain
        .chars()
        .filter(|c| !(c.is_alphanumeric() || c == &'-'))
        .count()
        > 0
    {
        error!("invalid client hello: only alphanumeric/hyphen chars allowed!");
        let data = serde_json::to_vec(&ServerHello::InvalidSubDomain).unwrap_or_default();
        let _ = websocket.send(Message::binary(data)).await;
        return None;
    }

    // ensure it's not a restricted one
    if CONFIG.blocked_sub_domains.contains(&sub_domain) {
        error!("invalid client hello: sub-domain restrict!");
        let data = serde_json::to_vec(&ServerHello::SubDomainInUse).unwrap_or_default();
        let _ = websocket.send(Message::binary(data)).await;
        return None;
    }

    // ensure this sub-domain isn't taken
    // check all instances
    match crate::network::instance_for_host(&sub_domain).await {
        Err(crate::network::Error::DoesNotServeHost) => {}
        Ok((_, existing_client)) => {
            if &existing_client != client_id {
                error!("invalid client hello: requested sub domain in use already!");
                let data = serde_json::to_vec(&ServerHello::SubDomainInUse).unwrap_or_default();
                let _ = websocket.send(Message::binary(data)).await;
                return None;
            }
        }
        Err(e) => {
            tracing::debug!("Got error checking instances: {:?}", e);
        }
    }

    Some((websocket, sub_domain))
}
