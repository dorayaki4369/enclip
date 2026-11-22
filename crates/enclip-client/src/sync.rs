//! アイテムの送信と受信（仕様 4.2、4.3）。
//!
//! 受信はサーバーの連番で取りこぼしなく進める。WebSocket の通知は「新しいものがある」合図として使い、
//! 中身は `fetch_new` で連番順に取り直す。

use enclip_core::{CoreError, DevicePublic, GroupSecrets, ItemId, ItemPayload, seal_item};
use thiserror::Error;

use crate::api::{ApiError, GroupAuth, ServerClient};
use crate::state::GroupState;

pub use crate::api::ItemSocket as ItemStream;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error(transparent)]
    Protocol(#[from] CoreError),
}

/// 受け入れたアイテム。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    pub seq: u64,
    pub id: ItemId,
    pub payload: ItemPayload,
    /// この端末自身が送ったもの。
    pub from_self: bool,
}

/// テキストを暗号化して送る。
pub async fn send_text(
    client: &ServerClient,
    group: &GroupSecrets,
    sender: &DevicePublic,
    text: &str,
    now_ms: u64,
    ttl_ms: u64,
) -> Result<u64, SyncError> {
    let payload = ItemPayload {
        text: text.to_owned(),
        sensitive: false,
        sender: sender.device_id(),
        created_ms: now_ms,
        expires_ms: now_ms.saturating_add(ttl_ms),
    };
    let envelope = seal_item(group, &payload)?;
    Ok(client
        .post_item(&GroupAuth::from_secrets(group), &envelope)
        .await?)
}

/// 取得済みの連番より後のアイテムをすべて取り、受け入れたものを返す。
/// 検証に失敗したもの、期限切れ、受け入れ済みのものは捨てて先へ進む。
/// 呼び出し側は戻った後に `state` を保存する。
pub async fn fetch_new(
    client: &ServerClient,
    state: &mut GroupState,
    me: &DevicePublic,
    now_ms: u64,
) -> Result<Vec<Received>, SyncError> {
    let secrets = state.secrets()?;
    let auth = GroupAuth::from_secrets(&secrets);
    let my_id = me.device_id();
    let mut received = Vec::new();
    loop {
        let page = client.items_after(&auth, state.cursor).await?;
        if page.is_empty() {
            return Ok(received);
        }
        for item in page {
            state.cursor = state.cursor.max(item.seq);
            if let Ok(accepted) = state.receive.accept(&secrets, &item.envelope, now_ms) {
                received.push(Received {
                    seq: item.seq,
                    id: accepted.id,
                    from_self: accepted.payload.sender == my_id,
                    payload: accepted.payload,
                });
            }
        }
    }
}
