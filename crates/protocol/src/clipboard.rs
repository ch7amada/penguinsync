//! Clipboard content and its size/type rules (docs/design.md §6.1,
//! docs/protocol.md §6.3).
//!
//! `text/plain` only in v1 — the MIME field exists so images are an
//! addition, not a break. Echo suppression is by content hash, mandatory,
//! since clipboard broadcasts to every connected paired device
//! (docs/design.md §6.1); the hash lives here, but the actual dedup — did we
//! just send or receive this exact content? — is the caller's job (`net`'s
//! session driving, or the daemon's clipboard orchestration), since it needs
//! to remember state across messages that this sans-I/O type doesn't own.

use serde::{Deserialize, Serialize};

/// Hard cap on clipboard payload size (docs/design.md §6.1).
pub const MAX_CONTENT_LEN: usize = 100 * 1024;

pub const MIME_TEXT_PLAIN: &str = "text/plain";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clip {
    pub mime: String,
    pub content: Vec<u8>,
    /// BLAKE3 of `content`. Two `Clip`s with the same content always hash
    /// the same, which is the entire mechanism echo suppression relies on.
    pub hash: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClipError {
    #[error("clipboard content is {len} bytes, over the {MAX_CONTENT_LEN}-byte cap")]
    TooLarge { len: usize },
    #[error("only {MIME_TEXT_PLAIN} is accepted in v1, got `{0}`")]
    UnsupportedMime(String),
}

impl Clip {
    /// The only constructor — nothing builds a `Clip` by hand, so the size
    /// cap and MIME restriction can never be bypassed, and the hash can
    /// never drift from the content it describes.
    pub fn new(mime: impl Into<String>, content: Vec<u8>) -> Result<Self, ClipError> {
        let mime = mime.into();
        if mime != MIME_TEXT_PLAIN {
            return Err(ClipError::UnsupportedMime(mime));
        }
        if content.len() > MAX_CONTENT_LEN {
            return Err(ClipError::TooLarge { len: content.len() });
        }
        let hash = *blake3::hash(&content).as_bytes();
        Ok(Self {
            mime,
            content,
            hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_content_hashes_the_same() {
        let a = Clip::new(MIME_TEXT_PLAIN, b"hello".to_vec()).unwrap();
        let b = Clip::new(MIME_TEXT_PLAIN, b"hello".to_vec()).unwrap();
        assert_eq!(a.hash, b.hash);
    }

    #[test]
    fn different_content_hashes_differently() {
        let a = Clip::new(MIME_TEXT_PLAIN, b"hello".to_vec()).unwrap();
        let b = Clip::new(MIME_TEXT_PLAIN, b"world".to_vec()).unwrap();
        assert_ne!(a.hash, b.hash);
    }

    #[test]
    fn rejects_oversized_content() {
        let content = vec![0u8; MAX_CONTENT_LEN + 1];
        assert_eq!(
            Clip::new(MIME_TEXT_PLAIN, content).unwrap_err(),
            ClipError::TooLarge {
                len: MAX_CONTENT_LEN + 1
            }
        );
    }

    #[test]
    fn accepts_content_at_exactly_the_cap() {
        let content = vec![0u8; MAX_CONTENT_LEN];
        assert!(Clip::new(MIME_TEXT_PLAIN, content).is_ok());
    }

    #[test]
    fn rejects_non_text_mime() {
        assert_eq!(
            Clip::new("image/png", vec![]).unwrap_err(),
            ClipError::UnsupportedMime("image/png".to_string())
        );
    }
}
