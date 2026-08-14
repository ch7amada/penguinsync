//! The paired-peer list, written to the app-private directory Kotlin passes
//! in at startup. Rust owns all persistent state on Android — this, the
//! device identity, and pinned peer keys are the only things written to disk
//! (docs/design.md §4.6). Kotlin's DataStore holds UI preferences only.
//!
//! Same shape as the daemon's `devices.json`, kept as a separate small copy
//! rather than a shared crate — the two owners (daemon, Android app) persist
//! to different directories under different lifecycle rules, and the whole
//! struct is a dozen lines.

use std::path::Path;

use serde::{Deserialize, Serialize};

use penguinsync_protocol::DeviceId;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedPeer {
    device_id: String,
    name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedPeers {
    peers: Vec<PersistedPeer>,
}

impl PersistedPeers {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)
    }

    pub fn upsert(&mut self, device_id: &DeviceId, name: &str) {
        let hex = penguinsync_protocol::message::to_hex(device_id);
        match self.peers.iter_mut().find(|p| p.device_id == hex) {
            Some(p) => p.name = name.to_string(),
            None => self.peers.push(PersistedPeer {
                device_id: hex,
                name: name.to_string(),
            }),
        }
    }

    pub fn device_ids(&self) -> impl Iterator<Item = DeviceId> + '_ {
        self.peers
            .iter()
            .filter_map(|p| penguinsync_protocol::message::from_hex(&p.device_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join(format!(
            "penguinsync-ffi-state-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("peers.json");

        let mut state = PersistedPeers::default();
        state.upsert(&[4u8; 32], "desk-fedora");
        state.save(&path).unwrap();

        let loaded = PersistedPeers::load(&path);
        assert_eq!(loaded.device_ids().collect::<Vec<_>>(), vec![[4u8; 32]]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_loads_empty() {
        let peers = PersistedPeers::load(Path::new("/nonexistent/penguinsync-ffi-test.json"));
        assert_eq!(peers.device_ids().count(), 0);
    }
}
