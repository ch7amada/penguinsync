//! File transfer metadata and name sanitization (docs/design.md §6.2,
//! docs/protocol.md §6.4).
//!
//! Metadata travels on the control stream; the payload itself travels on its
//! own unidirectional stream, prefixed with `transfer_id` and `offset`
//! (`net`'s job — this crate never touches a socket). [`sanitize_name`] is
//! here rather than in `net` because it's a pure function worth unit-testing
//! without a filesystem: the receiver never trusts a peer-supplied name
//! outright, since it's about to become a path component (docs/design.md
//! §6.2 — never overwrite, and never let a malicious peer write outside the
//! destination directory).

use serde::{Deserialize, Serialize};

/// Announces an incoming file, sent once per transfer before its payload
/// stream opens (docs/protocol.md §6.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferMeta {
    /// Correlates this announcement with the unidirectional stream carrying
    /// the bytes — the two arrive as independent QUIC streams with no
    /// ordering guarantee between them, so both sides need a shared id.
    pub transfer_id: u64,
    /// As presented by the sender. Never trusted as a path outright — see
    /// [`sanitize_name`].
    pub name: String,
    pub size: u64,
    /// Always 0 in v1. Reserved so resume is a protocol addition, not a
    /// protocol break (docs/design.md §6.2).
    pub offset: u64,
    /// BLAKE3 of the full file content, verified by the receiver on arrival.
    pub hash: [u8; 32],
}

/// Turns a peer-supplied file name into a safe path component, or `None` if
/// there isn't a reasonable one left after stripping it down.
///
/// Takes only the final path segment (a peer sending `../../etc/passwd` or
/// `a/b` gets `passwd` / `b`), and rejects the result if it's empty, `.`, or
/// `..` — those would either write nowhere useful or escape the destination
/// directory the caller has in mind.
pub fn sanitize_name(name: &str) -> Option<String> {
    let candidate = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    if candidate.is_empty() || candidate == "." || candidate == ".." {
        None
    } else {
        Some(candidate.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_name_passes_through() {
        assert_eq!(sanitize_name("photo.jpg"), Some("photo.jpg".to_string()));
    }

    #[test]
    fn path_separators_are_stripped_to_the_last_segment() {
        assert_eq!(sanitize_name("a/b/c.txt"), Some("c.txt".to_string()));
        assert_eq!(sanitize_name("a\\b\\c.txt"), Some("c.txt".to_string()));
    }

    #[test]
    fn traversal_attempts_are_neutralised() {
        assert_eq!(
            sanitize_name("../../etc/passwd"),
            Some("passwd".to_string())
        );
    }

    #[test]
    fn empty_or_dot_names_are_rejected() {
        assert_eq!(sanitize_name(""), None);
        assert_eq!(sanitize_name("."), None);
        assert_eq!(sanitize_name(".."), None);
        assert_eq!(sanitize_name("a/.."), None);
        assert_eq!(sanitize_name("a/"), None);
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(
            sanitize_name("  photo.jpg  "),
            Some("photo.jpg".to_string())
        );
    }
}
