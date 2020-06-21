use tunnelto::{ClientHelloV1, ClientHello, ClientId, ServerHello, ClientType};
use warp::filters::ws::{WebSocket, Message};
use futures::{SinkExt, StreamExt};
use crate::connected_clients::Connections;
use crate::auth_db::AuthResult;
use log::error;

pub struct ClientHandshake {
    pub id: ClientId,
    pub sub_domain: String,
    pub is_anonymous: bool,
}

pub async fn auth_client_handshake(mut websocket: WebSocket) -> Option<(WebSocket, ClientHandshake)> {
    let client_hello_data = match websocket.next().await {
        Some(Ok(msg)) => msg,
        _ => {
            error!("no client init message");
            return None
        },
    };

    if let Ok(client_hello_v1) = serde_json::from_slice::<ClientHelloV1>(client_hello_data.as_bytes()) {
        auth_client_v1(client_hello_v1, websocket).await
    } else {
        auth_client(client_hello_data.as_bytes(), websocket).await
    }
}

async fn auth_client_v1(client_hello: ClientHelloV1, mut websocket:WebSocket) -> Option<(WebSocket, ClientHandshake)> {
    let sub_domain = match  &client_hello.sub_domain {
        None => ServerHello::random_domain(),

        // otherwise, try to assign the sub domain
        Some(sub_domain) => {
            // ignore uppercase
            let sub_domain = sub_domain.to_lowercase();

            if sub_domain.chars().filter(|c| !c.is_alphanumeric()).count() > 0 {
                error!("invalid client hello: only alphanumeric chars allowed!");
                let data = serde_json::to_vec(&ServerHello::InvalidSubDomain).unwrap_or_default();
                let _ = websocket.send(Message::binary(data)).await;
                return None
            }

            // don't allow specified domains for anonymous v1 clients
            ServerHello::prefixed_random_domain(&sub_domain)
        }
    };

    Some((websocket, ClientHandshake {id: client_hello.id, sub_domain, is_anonymous: true}))
}

async fn auth_client(client_hello_data: &[u8], mut websocket: WebSocket) -> Option<(WebSocket, ClientHandshake)> {
    // parse the client hello
    let client_hello:ClientHello = match serde_json::from_slice(client_hello_data) {
        Ok(ch) => ch,
        Err(e) => {
            error!("invalid client hello: {}", e);
            let data = serde_json::to_vec(&ServerHello::AuthFailed).unwrap_or_default();
            let _ = websocket.send(Message::binary(data)).await;
            return None
        }
    };

    let (auth_key, requested_sub_domain) = match client_hello.client_type {
        ClientType::Anonymous =>  {
            let sub_domain = match client_hello.sub_domain {
                Some(sd) => ServerHello::prefixed_random_domain(&sd),
                None => ServerHello::random_domain(),
            };
            return Some((websocket, ClientHandshake { id: client_hello.id, sub_domain, is_anonymous: true }));
        },
        ClientType::Auth { key } => {
            match client_hello.sub_domain {
                Some(requested_sub_domain) => {
                    // ensure this sub-domain isn't taken
                    let existing_client = Connections::client_for_host(&requested_sub_domain);
                    if existing_client.is_some() && Some(&client_hello.id) != existing_client.as_ref() {
                        error!("invalid client hello: requested sub domain in use already!");
                        let data = serde_json::to_vec(&ServerHello::SubDomainInUse).unwrap_or_default();
                        let _ = websocket.send(Message::binary(data)).await;
                        return None
                    }

                    (key, requested_sub_domain)
                },
                None => {
                    let sub_domain = ServerHello::random_domain();
                    return Some((websocket, ClientHandshake { id: client_hello.id, sub_domain, is_anonymous: false }));
                }
            }
        }
    };


    // next authenticate the sub-domain
    let sub_domain = match crate::AUTH_DB_SERVICE.auth_sub_domain(&auth_key.0, &requested_sub_domain).await {
        Ok(AuthResult::Available) | Ok(AuthResult::ReservedByYou) => requested_sub_domain,
        Ok(AuthResult::ReservedByOther) => {
            let data = serde_json::to_vec(&ServerHello::SubDomainInUse).unwrap_or_default();
            let _ = websocket.send(Message::binary(data)).await;
            return None
        }
        Err(e) => {
            error!("error auth-ing user {:?}!", e);
            let data = serde_json::to_vec(&ServerHello::AuthFailed).unwrap_or_default();
            let _ = websocket.send(Message::binary(data)).await;
            return None
        }
    };

    Some((websocket, ClientHandshake { id: client_hello.id, sub_domain, is_anonymous: false }))
}

