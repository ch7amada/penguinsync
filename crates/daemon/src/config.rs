//! `$XDG_CONFIG_HOME/penguinsync/config.toml` — user, hand-editable
//! (docs/design.md §4.3).

use std::path::Path;

use serde::Deserialize;

/// Fixed rather than ephemeral, so a daemon restart doesn't invalidate every
/// paired device's cached reconnect address (docs/protocol.md §2 — reconnect
/// is unicast to a cached address, never mDNS). Unassigned by IANA.
pub const DEFAULT_PORT: u16 = 58210;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Name advertised to peers. Defaults to the machine's hostname.
    pub device_name: Option<String>,
    pub listen_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            device_name: None,
            listen_port: DEFAULT_PORT,
        }
    }
}

impl Config {
    /// Reads `path` if it exists; missing file or a parse error both fall
    /// back to defaults rather than refusing to start — a daemon that won't
    /// start over a config typo is a support nightmare, same reasoning as
    /// the GNOME extension being optional (docs/design.md §4.4).
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match toml::from_str(&text) {
            Ok(config) => config,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "malformed config, using defaults");
                Self::default()
            }
        }
    }

    pub fn device_name(&self) -> String {
        self.device_name.clone().unwrap_or_else(|| {
            hostname()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "penguinsync-device".to_string())
        })
    }
}

fn hostname() -> Option<String> {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .or_else(|| std::env::var("HOSTNAME").ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let config = Config::load(Path::new("/nonexistent/penguinsync-config-test.toml"));
        assert_eq!(config.listen_port, DEFAULT_PORT);
    }

    #[test]
    fn device_name_falls_back_when_unset() {
        let config = Config::default();
        assert!(!config.device_name().is_empty());
    }
}
