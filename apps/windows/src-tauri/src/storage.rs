//! 端末の状態と設定の保存。秘密を含む状態は OS の保護（Windows では DPAPI）をかけて保存する。

use std::path::PathBuf;

use enclip_client::{DeviceState, Url};
use serde::Deserialize;

use crate::platform;

const STATE_FILE: &str = "state.bin";
const CONFIG_FILE: &str = "config.json";
const SERVER_URL_ENV: &str = "ENCLIP_SERVER_URL";

pub struct Storage {
    dir: PathBuf,
}

#[derive(Deserialize)]
struct Config {
    server_url: String,
}

impl Storage {
    pub fn new(dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(Self { dir })
    }

    /// 中継サーバーの URL。環境変数、設定ファイルの順に探す。
    pub fn server_url(&self) -> Result<Url, String> {
        let text = match std::env::var(SERVER_URL_ENV) {
            Ok(url) => url,
            Err(_) => {
                let path = self.dir.join(CONFIG_FILE);
                let config = std::fs::read_to_string(&path).map_err(|_| {
                    format!(
                        "サーバーの URL がありません。{} に server_url を書いてください",
                        path.display()
                    )
                })?;
                serde_json::from_str::<Config>(&config)
                    .map_err(|e| e.to_string())?
                    .server_url
            }
        };
        Url::parse(&text).map_err(|e| e.to_string())
    }

    pub fn load_state(&self) -> Result<Option<DeviceState>, String> {
        let path = self.dir.join(STATE_FILE);
        if !path.exists() {
            return Ok(None);
        }
        let protected = std::fs::read(&path).map_err(|e| e.to_string())?;
        let plaintext = platform::unprotect(&protected)?;
        serde_json::from_slice(&plaintext)
            .map(Some)
            .map_err(|e| e.to_string())
    }

    pub fn save_state(&self, state: &DeviceState) -> Result<(), String> {
        let plaintext = serde_json::to_vec(state).map_err(|e| e.to_string())?;
        let protected = platform::protect(&plaintext)?;
        let path = self.dir.join(STATE_FILE);
        let temp = self.dir.join(format!("{STATE_FILE}.tmp"));
        std::fs::write(&temp, protected).map_err(|e| e.to_string())?;
        std::fs::rename(&temp, &path).map_err(|e| e.to_string())
    }
}
