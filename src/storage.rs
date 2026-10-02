use anyhow::Result;
use directories::ProjectDirs;
use std::path::PathBuf;

use crate::types::{Config, Track};

pub struct Storage {
    data_dir: PathBuf,
}

impl Storage {
    pub fn new() -> Result<Self> {
        let dirs = ProjectDirs::from("dev", "ytmusic", "ytmusic-rs")
            .ok_or_else(|| anyhow::anyhow!("Veri dizini bulunamadı"))?;
        let data_dir = dirs.data_local_dir().to_path_buf();
        std::fs::create_dir_all(&data_dir)?;
        Ok(Self { data_dir })
    }

    fn load<T: serde::de::DeserializeOwned + Default>(&self, file: &str) -> T {
        let path = self.data_dir.join(file);
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save<T: serde::Serialize + ?Sized>(&self, file: &str, data: &T) {
        let path = self.data_dir.join(file);
        if let Ok(json) = serde_json::to_string_pretty(data) {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn load_history(&self) -> Vec<Track> {
        self.load("history.json")
    }
    pub fn save_history(&self, v: &[Track]) {
        self.save("history.json", v);
    }

    pub fn load_favorites(&self) -> Vec<Track> {
        self.load("favorites.json")
    }
    pub fn save_favorites(&self, v: &[Track]) {
        self.save("favorites.json", v);
    }

    pub fn load_config(&self) -> Config {
        self.load("config.json")
    }
    pub fn save_config(&self, c: &Config) {
        self.save("config.json", c);
    }
}
