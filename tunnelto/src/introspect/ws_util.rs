use std::borrow::Cow;
use tokio_tungstenite::tungstenite::Message;
use tungstenite::protocol::CloseFrame;

pub fn warp_to_tung(
    message: warp::ws::Message,
) -> Result<tungstenite::Message, Box<dyn std::error::Error>> {
    let message = if message.is_binary() {
        Message::binary(message.into_bytes())
    } else if message.is_text() {
        Message::text(
            message
                .to_str()
                .expect("internal inconsistency: websockets")
                .to_string(),
        )
    } else if message.is_close() {
        let frame = message.close_frame().map(|(s, m)| CloseFrame {
            code: s.into(),
            reason: Cow::Owned(m.to_string()),
        });
        Message::Close(frame)
    } else if message.is_ping() {
        Message::Ping(message.into_bytes())
    } else if message.is_pong() {
        Message::Pong(message.into_bytes())
    } else {
        return Err("invalid message")?;
    };

    Ok(message)
}

pub fn tung_to_warp(
    message: tungstenite::Message,
) -> Result<warp::ws::Message, Box<dyn std::error::Error>> {
    use warp::ws::Message;

    let message = if message.is_binary() {
        Message::binary(message.into_data())
    } else if message.is_text() {
        Message::text(
            message
                .to_text()
                .expect("internal inconsistency: websockets")
                .to_string(),
        )
    } else if message.is_close() {
        Message::close()
    } else if message.is_ping() {
        Message::ping(message.into_data())
    } else if message.is_pong() {
        Message::pong(message.into_data())
    } else {
        return Err("invalid message")?;
    };

    Ok(message)
}
