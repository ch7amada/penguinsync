//! File transfer payload streams: the header shared by both directions, and
//! [`FsSink`], the one [`TransferSink`] implementation v1 needs (docs/design.md
//! §6.2, docs/protocol.md §6.4).
//!
//! Metadata (`TransferOffer`/`TransferComplete`) rides the control stream and
//! is [`crate::session`]'s job; this module only ever sees the unidirectional
//! payload stream — writing it on the sending side, reading and verifying it
//! on the receiving side.

use std::path::{Path, PathBuf};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Chunk size for both hashing passes and stream copies. Small enough to
/// keep memory flat regardless of file size, large enough that per-chunk
/// overhead doesn't dominate.
const CHUNK_LEN: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("reading local file: {0}")]
    ReadLocal(std::io::Error),
    #[error("writing received file: {0}")]
    WriteLocal(std::io::Error),
    #[error("stream write failed: {0}")]
    StreamWrite(#[from] quinn::WriteError),
    #[error("stream read failed: {0}")]
    StreamRead(#[from] quinn::ReadError),
    #[error("reading transfer stream header: {0}")]
    HeaderRead(#[from] quinn::ReadExactError),
    #[error("opening transfer stream: {0}")]
    OpenStream(#[from] quinn::ConnectionError),
    #[error("finishing transfer stream: {0}")]
    FinishStream(#[from] quinn::ClosedStream),
    #[error(
        "peer closed the stream before sending the full {expected}-byte file ({got} bytes received)"
    )]
    Truncated { expected: u64, got: u64 },
    #[error("BLAKE3 mismatch after receiving the full file — discarded")]
    HashMismatch,
    #[error("file name `{0}` is not safe to write")]
    UnsafeName(String),
    #[error("destination directory unavailable: {0}")]
    Destination(std::io::Error),
}

/// Everything [`FsSink::hash_and_stat`] needs to build a `TransferOffer`
/// before a single byte of the payload stream is sent.
pub struct LocalFile {
    pub size: u64,
    pub hash: [u8; 32],
}

/// Hashes and stats a local file in one read pass, for the sending side to
/// build a `TransferMeta` before opening the payload stream. A second read
/// pass (the actual send) follows — simple and correct; streaming the hash
/// alongside the send itself would save one pass but means the announcement
/// can't carry the final hash before the bytes start moving, which is the
/// shape docs/protocol.md §6.4 commits to. Real-hardware throughput
/// (docs/design.md §10 item 2) is a measurement task for later, not a
/// blocker here.
pub async fn hash_and_stat(path: &Path) -> Result<LocalFile, TransferError> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(TransferError::ReadLocal)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; CHUNK_LEN];
    let mut size = 0u64;
    loop {
        let n = file
            .read(&mut buf)
            .await
            .map_err(TransferError::ReadLocal)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok(LocalFile {
        size,
        hash: *hasher.finalize().as_bytes(),
    })
}

/// Writes `path`'s content to `send` in fixed-size chunks, calling
/// `on_progress` with the cumulative byte count after each one. The header
/// (`transfer_id` + `offset`) is the caller's job — this only ever writes
/// the payload.
pub async fn send_file_stream(
    path: &Path,
    send: &mut quinn::SendStream,
    mut on_progress: impl FnMut(u64),
) -> Result<(), TransferError> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(TransferError::ReadLocal)?;
    let mut buf = vec![0u8; CHUNK_LEN];
    let mut sent = 0u64;
    loop {
        let n = file
            .read(&mut buf)
            .await
            .map_err(TransferError::ReadLocal)?;
        if n == 0 {
            break;
        }
        send.write_all(&buf[..n]).await?;
        sent += n as u64;
        on_progress(sent);
    }
    Ok(())
}

/// Receives content and verifies it. Implementations decide destination and
/// naming; the one v1 needs is [`FsSink`].
#[async_trait::async_trait]
pub trait TransferSink: Send + Sync {
    /// Reads exactly `size` bytes from `recv`, verifies them against `hash`,
    /// and returns the path they ended up at. `on_progress` is called with
    /// the cumulative byte count as the transfer proceeds — coarse-grained
    /// (once per chunk) is enough for a progress bar, not a live byte
    /// counter.
    ///
    /// Never overwrites (docs/design.md §6.2): a name collision gets a
    /// `(1)`-style suffix. On any error — a bad name, a short read, a hash
    /// mismatch — the partial file is removed rather than kept; there is no
    /// resume in v1.
    async fn receive(
        &self,
        name: &str,
        size: u64,
        hash: [u8; 32],
        recv: quinn::RecvStream,
        on_progress: Box<dyn Fn(u64) + Send + Sync>,
    ) -> Result<PathBuf, TransferError>;
}

/// Writes into a fixed base directory on the local filesystem — the daemon's
/// `$XDG_DOWNLOAD_DIR/PenguinSync/` and, for now, the Android app's
/// data-directory-relative downloads folder (docs/design.md §6.2; proper
/// `MediaStore`/SAF integration on Android is a follow-up that needs a real
/// device to get right).
pub struct FsSink {
    base_dir: PathBuf,
}

impl FsSink {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// `name`, `name (1)`, `name (2)`, … — the first path under `base_dir`
    /// that doesn't already exist. `name` has already passed through
    /// [`penguinsync_protocol::transfer::sanitize_name`] by the time this is
    /// called.
    fn collision_safe_path(&self, name: &str) -> PathBuf {
        let candidate = self.base_dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
        let (stem, ext) = match name.rsplit_once('.') {
            // A leading-dot "name" (`.bashrc`) has no extension to split off.
            Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
            _ => (name, None),
        };
        for n in 1u32.. {
            let candidate_name = match ext {
                Some(ext) => format!("{stem} ({n}).{ext}"),
                None => format!("{stem} ({n})"),
            };
            let candidate = self.base_dir.join(candidate_name);
            if !candidate.exists() {
                return candidate;
            }
        }
        unreachable!("u32 exhausted looking for a free collision suffix")
    }
}

#[async_trait::async_trait]
impl TransferSink for FsSink {
    async fn receive(
        &self,
        name: &str,
        size: u64,
        hash: [u8; 32],
        mut recv: quinn::RecvStream,
        on_progress: Box<dyn Fn(u64) + Send + Sync>,
    ) -> Result<PathBuf, TransferError> {
        let safe_name = penguinsync_protocol::transfer::sanitize_name(name)
            .ok_or_else(|| TransferError::UnsafeName(name.to_string()))?;

        tokio::fs::create_dir_all(&self.base_dir)
            .await
            .map_err(TransferError::Destination)?;
        let dest = self.collision_safe_path(&safe_name);
        let mut tmp_name = dest
            .file_name()
            .expect("collision_safe_path always joins a file name")
            .to_os_string();
        tmp_name.push(".part");
        let tmp = dest.with_file_name(tmp_name);

        let result = receive_to_temp(&mut recv, &tmp, size, hash, on_progress).await;
        match result {
            Ok(()) => {
                tokio::fs::rename(&tmp, &dest)
                    .await
                    .map_err(TransferError::WriteLocal)?;
                Ok(dest)
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                Err(e)
            }
        }
    }
}

async fn receive_to_temp(
    recv: &mut quinn::RecvStream,
    tmp: &Path,
    size: u64,
    hash: [u8; 32],
    on_progress: Box<dyn Fn(u64) + Send + Sync>,
) -> Result<(), TransferError> {
    let mut file = tokio::fs::File::create(tmp)
        .await
        .map_err(TransferError::WriteLocal)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; CHUNK_LEN];
    let mut received = 0u64;

    loop {
        let n = match recv.read(&mut buf).await? {
            Some(n) => n,
            None => break, // peer's FIN
        };
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])
            .await
            .map_err(TransferError::WriteLocal)?;
        received += n as u64;
        on_progress(received);
    }

    if received != size {
        return Err(TransferError::Truncated {
            expected: size,
            got: received,
        });
    }
    if *hasher.finalize().as_bytes() != hash {
        return Err(TransferError::HashMismatch);
    }
    file.flush().await.map_err(TransferError::WriteLocal)?;
    Ok(())
}

/// Payload stream header: `transfer_id` then `offset`, both little-endian
/// `u64` (docs/protocol.md §6.4). No length prefix — `size` from the
/// `TransferOffer` and the stream's own EOF delimit the payload.
pub async fn write_header(
    send: &mut quinn::SendStream,
    transfer_id: u64,
    offset: u64,
) -> Result<(), TransferError> {
    send.write_all(&transfer_id.to_le_bytes()).await?;
    send.write_all(&offset.to_le_bytes()).await?;
    Ok(())
}

/// Returns `(transfer_id, offset)`.
pub async fn read_header(recv: &mut quinn::RecvStream) -> Result<(u64, u64), TransferError> {
    let mut buf = [0u8; 16];
    recv.read_exact(&mut buf).await?;
    let transfer_id = u64::from_le_bytes(buf[0..8].try_into().expect("8 bytes"));
    let offset = u64::from_le_bytes(buf[8..16].try_into().expect("8 bytes"));
    Ok((transfer_id, offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "penguinsync-fssink-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn collision_safe_path_appends_numeric_suffix() {
        let dir = temp_dir("collision");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("photo.jpg"), b"first").unwrap();

        let sink = FsSink::new(&dir);
        let path = sink.collision_safe_path("photo.jpg");
        assert_eq!(path, dir.join("photo (1).jpg"));

        std::fs::write(&path, b"second").unwrap();
        let path2 = sink.collision_safe_path("photo.jpg");
        assert_eq!(path2, dir.join("photo (2).jpg"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_collision_uses_the_plain_name() {
        let dir = temp_dir("no-collision");
        let sink = FsSink::new(&dir);
        assert_eq!(sink.collision_safe_path("photo.jpg"), dir.join("photo.jpg"));
    }

    #[test]
    fn collision_on_an_extensionless_name_still_gets_a_suffix() {
        let dir = temp_dir("no-ext");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("README"), b"x").unwrap();

        let sink = FsSink::new(&dir);
        assert_eq!(sink.collision_safe_path("README"), dir.join("README (1)"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn hash_and_stat_matches_a_streamed_hash() {
        let dir = temp_dir("hash-stat");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.bin");
        let content = vec![7u8; CHUNK_LEN * 2 + 10];
        std::fs::write(&path, &content).unwrap();

        let stat = hash_and_stat(&path).await.unwrap();
        assert_eq!(stat.size, content.len() as u64);
        assert_eq!(stat.hash, *blake3::hash(&content).as_bytes());

        std::fs::remove_dir_all(&dir).ok();
    }
}
