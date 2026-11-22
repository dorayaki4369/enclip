//! enclip の中継サーバーと話す端末側の処理。
//!
//! Windows アプリと開発用 CLI で共有する。OS との連携と保存は持たない。
//! サーバーの API は `apps/server` を正とする。

mod api;
mod pairing;
mod state;
mod sync;

pub use api::{ApiError, GroupAuth, ServerClient};
pub use pairing::{InviteSession, JoinSession, PairingError};
pub use state::{DeviceState, GroupState};
pub use sync::{ItemStream, Received, SyncError, fetch_new, send_text};
pub use url::Url;
