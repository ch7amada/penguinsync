//! Length-prefixed message framing over a QUIC stream.
//!
//! A QUIC stream is a reliable ordered byte stream, not a message stream, so
//! message boundaries still need marking. A `u32` little-endian byte length
//! header is the entire scheme — `postcard`'s encoding is not
//! self-delimiting on its own.

use penguinsync_protocol::Message;

/// Refuse to allocate for a claimed length past this. Generous for control
/// messages (handshake, ping/pong); a deliberate guard against a peer
/// claiming a multi-gigabyte frame and exhausting memory before any content
/// has even been authenticated by the app layer.
const MAX_FRAME_LEN: u32 = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FramingError {
    #[error("frame length {0} exceeds the {MAX_FRAME_LEN}-byte limit")]
    FrameTooLarge(u32),
    #[error("stream write failed: {0}")]
    Write(#[from] quinn::WriteError),
    #[error("stream read failed: {0}")]
    Read(#[from] quinn::ReadExactError),
    #[error("decoding message: {0}")]
    Decode(#[from] penguinsync_protocol::message::CodecError),
}

pub async fn write_message(
    stream: &mut quinn::SendStream,
    msg: &Message,
) -> Result<(), FramingError> {
    let bytes = penguinsync_protocol::message::encode(msg).expect("Message always encodes");
    let len = u32::try_from(bytes.len()).expect("control messages are far below u32::MAX");
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(&bytes).await?;
    Ok(())
}

pub async fn read_message(stream: &mut quinn::RecvStream) -> Result<Message, FramingError> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf);
    if len > MAX_FRAME_LEN {
        return Err(FramingError::FrameTooLarge(len));
    }
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf).await?;
    Ok(penguinsync_protocol::message::decode(&buf)?)
}
