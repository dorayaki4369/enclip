//! 同期の中心。利用者のコピーを送り、届いたアイテムを履歴に入れて、条件を満たせばクリップボードへ反映する。
//!
//! 自動反映の条件（合意済み）: 届いたアイテムの作成時刻が、この端末で最後にコピーした時刻
//! （または最後に反映したアイテムの作成時刻）より新しい場合だけ、最新の 1 件を反映する。

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use enclip_client::{
    DeviceState, GroupAuth, GroupState, InviteSession, Received, ServerClient, fetch_new, send_text,
};
use enclip_core::GroupSecrets;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, Notify};

use crate::clipboard_worker::ClipboardHandle;
use crate::storage::Storage;

/// 普通のテキストを残す期間（PoC の固定値）。
const ITEM_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const HISTORY_LIMIT: usize = 50;
const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(30);

#[derive(Clone, Serialize)]
pub struct HistoryEntry {
    pub text: String,
    pub from_self: bool,
    pub created_ms: u64,
    pub expires_ms: u64,
}

#[derive(Clone, Serialize)]
pub struct StatusView {
    pub connected: bool,
    pub paused: bool,
    pub error: Option<String>,
    pub history: Vec<HistoryEntry>,
}

struct Inner {
    state: DeviceState,
    history: Vec<HistoryEntry>,
    paused: bool,
    connected: bool,
    error: Option<String>,
    /// この端末のクリップボードの内容がいつの時点のものか（UNIX ミリ秒）。
    clipboard_as_of_ms: u64,
    /// 送れなかったコピー。接続が戻ったら送る。
    pending: Vec<(String, u64)>,
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Mutex<Inner>>,
    invite: Arc<Mutex<Option<InviteSession>>>,
    /// 参加申請を待ち受ける処理。ペアリング画面を閉じたら止める。
    invite_wait: Arc<std::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    client: ServerClient,
    storage: Arc<Storage>,
    clipboard: ClipboardHandle,
    app: AppHandle,
    wake: Arc<Notify>,
}

impl Engine {
    /// 状態を読み込む。初回は端末鍵とグループを作る（グループは Windows で作る合意）。
    pub fn load(
        app: AppHandle,
        storage: Storage,
        clipboard: ClipboardHandle,
    ) -> Result<Self, String> {
        let client = ServerClient::new(storage.server_url()?);
        let state = match storage.load_state()? {
            Some(state) => state,
            None => {
                let mut state = DeviceState::generate().map_err(|e| e.to_string())?;
                let secrets = GroupSecrets::create().map_err(|e| e.to_string())?;
                state.group = Some(GroupState::new(&secrets));
                storage.save_state(&state)?;
                state
            }
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                state,
                history: Vec::new(),
                paused: false,
                connected: false,
                error: None,
                clipboard_as_of_ms: now_ms(),
                pending: Vec::new(),
            })),
            invite: Arc::new(Mutex::new(None)),
            invite_wait: Arc::new(std::sync::Mutex::new(None)),
            client,
            storage: Arc::new(storage),
            clipboard,
            app,
            wake: Arc::new(Notify::new()),
        })
    }

    pub async fn status(&self) -> StatusView {
        let inner = self.inner.lock().await;
        StatusView {
            connected: inner.connected,
            paused: inner.paused,
            error: inner.error.clone(),
            history: visible_history(&inner.history, now_ms()),
        }
    }

    pub async fn set_paused(&self, paused: bool) {
        self.inner.lock().await.paused = paused;
        self.notify_changed();
    }

    pub async fn is_paused(&self) -> bool {
        self.inner.lock().await.paused
    }

    /// 履歴の項目をクリップボードへ入れる。
    pub async fn copy_history(&self, index: usize) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        let entry = visible_history(&inner.history, now_ms())
            .get(index)
            .ok_or("no such item")?
            .clone();
        inner.clipboard_as_of_ms = now_ms();
        self.clipboard.write(entry.text);
        Ok(())
    }

    /// 利用者のコピーを受けて送る。
    pub async fn on_local_copy(&self, text: String) {
        let created_ms = now_ms();
        {
            let mut inner = self.inner.lock().await;
            // 一時停止中のコピーは送らないが、自動反映の判定には使う。
            inner.clipboard_as_of_ms = created_ms;
            if inner.paused {
                return;
            }
            inner.pending.push((text, created_ms));
        }
        self.flush_pending().await;
    }

    async fn flush_pending(&self) {
        let (pending, secrets, me) = {
            let mut inner = self.inner.lock().await;
            let Some(secrets) = inner.state.group.as_ref().and_then(|g| g.secrets().ok()) else {
                return;
            };
            let Ok(me) = inner.state.device_key().map(|k| k.public()) else {
                return;
            };
            (std::mem::take(&mut inner.pending), secrets, me)
        };
        let ttl = duration_ms(ITEM_TTL);
        let mut failed = Vec::new();
        for (text, created_ms) in pending {
            if let Err(error) = send_text(&self.client, &secrets, &me, &text, created_ms, ttl).await
            {
                self.set_error(Some(format!("送信に失敗しました: {error}")))
                    .await;
                failed.push((text, created_ms));
            }
        }
        if !failed.is_empty() {
            let mut inner = self.inner.lock().await;
            failed.append(&mut inner.pending);
            inner.pending = failed;
        }
    }

    /// サーバーとの接続を保ち、届いたアイテムを処理し続ける。
    pub async fn run_sync(self) {
        let mut backoff = RECONNECT_MIN;
        loop {
            match self.sync_once().await {
                Ok(()) => backoff = RECONNECT_MIN,
                Err(error) => {
                    self.set_error(Some(error)).await;
                }
            }
            self.inner.lock().await.connected = false;
            self.notify_changed();
            tokio::select! {
                () = tokio::time::sleep(backoff) => {}
                () = self.wake.notified() => {}
            }
            backoff = (backoff * 2).min(RECONNECT_MAX);
        }
    }

    async fn sync_once(&self) -> Result<(), String> {
        let auth = {
            let inner = self.inner.lock().await;
            let group = inner.state.group.as_ref().ok_or("グループがありません")?;
            GroupAuth::from_secrets(&group.secrets().map_err(|e| e.to_string())?)
        };
        self.client
            .register_group(&auth)
            .await
            .map_err(|e| format!("サーバーに接続できません: {e}"))?;
        let mut socket = self
            .client
            .subscribe(&auth)
            .await
            .map_err(|e| e.to_string())?;
        {
            let mut inner = self.inner.lock().await;
            inner.connected = true;
            inner.error = None;
        }
        self.notify_changed();
        self.flush_pending().await;
        self.fetch_and_apply().await?;
        while let Some(message) = socket.next().await {
            message.map_err(|e| e.to_string())?;
            self.fetch_and_apply().await?;
        }
        Ok(())
    }

    async fn fetch_and_apply(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        let me = inner
            .state
            .device_key()
            .map_err(|e| e.to_string())?
            .public();
        let group = inner.state.group.as_mut().ok_or("グループがありません")?;
        let received = fetch_new(&self.client, group, &me, now_ms())
            .await
            .map_err(|e| e.to_string())?;
        if received.is_empty() {
            return Ok(());
        }
        self.storage.save_state(&inner.state)?;
        let to_apply = item_to_apply(&received, inner.clipboard_as_of_ms, inner.paused)
            .map(|item| (item.payload.text.clone(), item.payload.created_ms));
        for item in &received {
            inner.history.insert(
                0,
                HistoryEntry {
                    text: item.payload.text.clone(),
                    from_self: item.from_self,
                    created_ms: item.payload.created_ms,
                    expires_ms: item.payload.expires_ms,
                },
            );
        }
        let now = now_ms();
        inner.history = visible_history(&inner.history, now);
        if let Some((text, created_ms)) = to_apply {
            inner.clipboard_as_of_ms = created_ms;
            self.clipboard.write(text);
        }
        drop(inner);
        self.notify_changed();
        Ok(())
    }

    /// 端末の追加を始め、QR コードの文字列を返す。参加申請が届いたら `pairing-code` を通知する。
    pub async fn start_invite(&self) -> Result<String, String> {
        let (device, group) = {
            let inner = self.inner.lock().await;
            let device = inner.state.device_key().map_err(|e| e.to_string())?;
            let group = inner
                .state
                .group
                .as_ref()
                .ok_or("グループがありません")?
                .secrets()
                .map_err(|e| e.to_string())?;
            (device, group)
        };
        self.cancel_invite().await;
        let session = InviteSession::start(self.client.clone(), device, group)
            .await
            .map_err(|e| e.to_string())?;
        let qr = session.qr_text();
        *self.invite.lock().await = Some(session);

        let engine = self.clone();
        let wait = tauri::async_runtime::spawn(async move {
            let mut guard = engine.invite.lock().await;
            let Some(session) = guard.as_mut() else {
                return;
            };
            let result = session.wait_for_join().await;
            drop(guard);
            let event = match result {
                Ok(code) => PairingEvent::Code { code: code.0 },
                Err(error) => PairingEvent::Failed {
                    message: error.to_string(),
                },
            };
            let _ = engine.app.emit("pairing", event);
        });
        *self.invite_wait.lock().expect("lock is not poisoned") = Some(wait);
        Ok(qr)
    }

    pub async fn approve_invite(&self) -> Result<(), String> {
        let mut guard = self.invite.lock().await;
        let session = guard.as_mut().ok_or("ペアリング中ではありません")?;
        session.approve().await.map_err(|e| e.to_string())?;
        *guard = None;
        Ok(())
    }

    /// ペアリング画面を閉じたときに呼ぶ。待ち受け中の処理も止める。
    pub async fn cancel_invite(&self) {
        // 待ち受けの処理がセッションのロックを持つので、先に止めてからセッションを取り出す。
        if let Some(wait) = self
            .invite_wait
            .lock()
            .expect("lock is not poisoned")
            .take()
        {
            wait.abort();
        }
        let session = self.invite.lock().await.take();
        if let Some(session) = session {
            let _ = session.cancel().await;
        }
    }

    async fn set_error(&self, error: Option<String>) {
        self.inner.lock().await.error = error;
        self.notify_changed();
    }

    fn notify_changed(&self) {
        let _ = self.app.emit("status-changed", ());
    }
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PairingEvent {
    Code { code: String },
    Failed { message: String },
}

/// 表示する履歴。期限を過ぎたものを除き（仕様 4.4）、暗号文の中の作成時刻の新しい順に並べる（仕様 4.3）。
fn visible_history(history: &[HistoryEntry], now_ms: u64) -> Vec<HistoryEntry> {
    let mut visible: Vec<HistoryEntry> = history
        .iter()
        .filter(|entry| now_ms < entry.expires_ms)
        .cloned()
        .collect();
    visible.sort_by_key(|entry| std::cmp::Reverse(entry.created_ms));
    visible.truncate(HISTORY_LIMIT);
    visible
}

/// クリップボードへ自動で反映するアイテム。
/// 他の端末から届いた中で最も新しいものが、今のクリップボードの内容より新しい場合だけ選ぶ。
fn item_to_apply(
    received: &[Received],
    clipboard_as_of_ms: u64,
    paused: bool,
) -> Option<&Received> {
    if paused {
        return None;
    }
    received
        .iter()
        .filter(|item| !item.from_self)
        .max_by_key(|item| item.payload.created_ms)
        .filter(|item| item.payload.created_ms > clipboard_as_of_ms)
}

fn now_ms() -> u64 {
    duration_ms(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after 1970"),
    )
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).expect("duration fits in u64")
}

#[cfg(test)]
mod tests {
    use enclip_core::{ItemId, ItemPayload};

    use super::*;

    fn item(text: &str, created_ms: u64, from_self: bool) -> Received {
        Received {
            seq: created_ms,
            id: ItemId([0; 16]),
            payload: ItemPayload {
                text: text.to_owned(),
                sensitive: false,
                sender: [0; 16],
                created_ms,
                expires_ms: created_ms + 1,
            },
            from_self,
        }
    }

    #[test]
    fn applies_newest_item_from_other_devices() {
        let received = [
            item("old", 10, false),
            item("new", 30, false),
            item("mine", 40, true),
        ];
        assert_eq!(
            item_to_apply(&received, 5, false).map(|i| i.payload.text.as_str()),
            Some("new")
        );
    }

    #[test]
    fn does_not_overwrite_a_newer_local_copy() {
        // オフライン中に届いていたアイテムより後に、この端末でコピーしていた場合。
        let received = [item("from phone", 20, false)];
        assert!(item_to_apply(&received, 25, false).is_none());
    }

    #[test]
    fn history_is_ordered_by_creation_and_drops_expired() {
        let entry = |text: &str, created_ms: u64, expires_ms: u64| HistoryEntry {
            text: text.to_owned(),
            from_self: false,
            created_ms,
            expires_ms,
        };
        let history = [
            entry("late arrival", 10, 100),
            entry("newer", 20, 100),
            entry("expired", 30, 50),
        ];
        let texts: Vec<_> = visible_history(&history, 60)
            .into_iter()
            .map(|e| e.text)
            .collect();
        assert_eq!(texts, ["newer", "late arrival"]);
    }

    #[test]
    fn does_not_apply_while_paused_or_from_self() {
        assert!(item_to_apply(&[item("a", 20, false)], 0, true).is_none());
        assert!(item_to_apply(&[item("a", 20, true)], 0, false).is_none());
    }
}
