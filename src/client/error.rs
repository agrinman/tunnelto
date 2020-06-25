use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to connect to control server: {0}.")]
    WebSocketError(#[from] tungstenite::error::Error),

    #[error("Server denied the connection. Please check your authentication key.")]
    AuthenticationFailed,

    #[error("Invalid sub-domain specified.")]
    InvalidSubDomain,

    #[error("Cannot use this sub-domain, it is already taken.")]
    SubDomainInUse,

    #[error("The server responded with an invalid response.")]
    ServerReplyInvalid,

    #[error("The server did not respond to our client_hello.")]
    NoResponseFromServer,

}