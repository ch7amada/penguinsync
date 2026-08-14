//! `$XDG_DATA_HOME/penguinsync/devices.json` — daemon-owned, never
//! hand-edited (docs/design.md §4.3).
//!
//! Behind a plain struct with an explicit `load`/`save`, so swapping to
//! SQLite when transfer history arrives is an implementation change, not an
//! API change (docs/design.md §4.3).

use std::path::Path;

use serde::{Deserialize, Serialize};

use penguinsync_protocol::DeviceId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedDevice {
    pub device_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedState {
    pub devices: Vec<PersistedDevice>,
}

impl PersistedState {
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match serde_json::from_str(&text) {
            Ok(state) => state,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "malformed device state, starting empty");
                Self::default()
            }
        }
    }

    /// Writes via a temp file + rename so a crash mid-write can never leave
    /// `devices.json` truncated.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn upsert(&mut self, device_id: &DeviceId, name: &str) {
        let hex = penguinsync_protocol::message::to_hex(device_id);
        match self.devices.iter_mut().find(|d| d.device_id == hex) {
            Some(d) => d.name = name.to_string(),
            None => self.devices.push(PersistedDevice {
                device_id: hex,
                name: name.to_string(),
            }),
        }
    }

    pub fn remove(&mut self, device_id: &DeviceId) {
        let hex = penguinsync_protocol::message::to_hex(device_id);
        self.devices.retain(|d| d.device_id != hex);
    }

    pub fn device_ids(&self) -> impl Iterator<Item = DeviceId> + '_ {
        self.devices
            .iter()
            .filter_map(|d| penguinsync_protocol::message::from_hex(&d.device_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join(format!(
            "penguinsync-state-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("devices.json");

        let mut state = PersistedState::default();
        state.upsert(&[3u8; 32], "pixel");
        state.save(&path).unwrap();

        let loaded = PersistedState::load(&path);
        assert_eq!(loaded.devices.len(), 1);
        assert_eq!(loaded.devices[0].name, "pixel");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn upsert_replaces_existing_by_id() {
        let mut state = PersistedState::default();
        state.upsert(&[1u8; 32], "first-name");
        state.upsert(&[1u8; 32], "renamed");
        assert_eq!(state.devices.len(), 1);
        assert_eq!(state.devices[0].name, "renamed");
    }

    #[test]
    fn remove_drops_the_device() {
        let mut state = PersistedState::default();
        state.upsert(&[1u8; 32], "a");
        state.remove(&[1u8; 32]);
        assert!(state.devices.is_empty());
    }
}
