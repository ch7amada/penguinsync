//! Reconnect backoff.
//!
//! Exponential, capped at ~60 s (docs/design.md §5.3, §9 — M0's "survive a
//! Wi-Fi drop and reconnect"). A pure function of the attempt count so the
//! schedule is testable without a clock or a socket.

use std::time::Duration;

const BASE: Duration = Duration::from_secs(1);
const CAP: Duration = Duration::from_secs(60);

/// Delay before reconnect attempt number `attempt` (0-indexed: the first
/// retry after a drop is `attempt = 0`).
pub fn delay(attempt: u32) -> Duration {
    let factor = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
    BASE.checked_mul(factor).unwrap_or(CAP).min(CAP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_base_and_doubles() {
        assert_eq!(delay(0), Duration::from_secs(1));
        assert_eq!(delay(1), Duration::from_secs(2));
        assert_eq!(delay(2), Duration::from_secs(4));
        assert_eq!(delay(3), Duration::from_secs(8));
    }

    #[test]
    fn caps_at_sixty_seconds() {
        assert_eq!(delay(10), CAP);
        assert_eq!(delay(1000), CAP);
        assert_eq!(delay(u32::MAX), CAP);
    }
}
