//! アイテムの暗号化（仕様 4.2）。

use serde::{Deserialize, Serialize};

use crate::encoding::{b64_array, b64_vec};
use crate::primitives::{aead_open, aead_seal, random_array};
use crate::{CoreError, GroupId, GroupSecrets};

const BODY_LABEL: &[u8] = b"enclip/item/body/v1";
const KEY_LABEL: &[u8] = b"enclip/item/key/v1";

/// アイテム ID。送る端末が乱数で作る。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ItemId(#[serde(with = "b64_array")] pub [u8; 16]);

/// 暗号文の中に入れる内容。期限と作成時刻はサーバーに改ざんされない。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemPayload {
    pub text: String,
    pub sensitive: bool,
    #[serde(with = "b64_array")]
    pub sender: [u8; 16],
    /// UNIX 時刻（ミリ秒）。
    pub created_ms: u64,
    /// UNIX 時刻（ミリ秒）。これ以降は受け入れない。
    pub expires_ms: u64,
}

/// サーバーに渡す形。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemEnvelope {
    pub id: ItemId,
    /// アイテム鍵で暗号化した内容。
    #[serde(with = "b64_vec")]
    pub body: Vec<u8>,
    /// ユーザー鍵で暗号化したアイテム鍵。
    #[serde(with = "b64_vec")]
    pub wrapped_key: Vec<u8>,
    /// サーバーが削除に使う期限の目安。端末は暗号文の中の期限だけを信じる。
    pub server_expires_ms: u64,
}

fn aad(label: &[u8], group_id: &GroupId, id: &ItemId) -> Vec<u8> {
    [label, &group_id.0, &id.0].concat()
}

/// 1 件ごとに新しいアイテム鍵とアイテム ID を作って暗号化する。
pub fn seal_item(group: &GroupSecrets, payload: &ItemPayload) -> Result<ItemEnvelope, CoreError> {
    let id = ItemId(random_array()?);
    let mut item_key: [u8; 32] = random_array()?;
    let plaintext = serde_json::to_vec(payload).map_err(|_| CoreError::Malformed("payload"))?;
    let body = aead_seal(
        &item_key,
        &plaintext,
        &aad(BODY_LABEL, &group.group_id, &id),
    )?;
    let wrapped_key = aead_seal(
        &group.user_key.0,
        &item_key,
        &aad(KEY_LABEL, &group.group_id, &id),
    )?;
    zeroize::Zeroize::zeroize(&mut item_key);
    Ok(ItemEnvelope {
        id,
        body,
        wrapped_key,
        server_expires_ms: payload.expires_ms,
    })
}

/// アイテム鍵、内容の順に復号する。関連データが合わなければ失敗する。
pub(crate) fn open_item(
    group: &GroupSecrets,
    envelope: &ItemEnvelope,
) -> Result<ItemPayload, CoreError> {
    let mut item_key: [u8; 32] = aead_open(
        &group.user_key.0,
        &envelope.wrapped_key,
        &aad(KEY_LABEL, &group.group_id, &envelope.id),
    )?
    .try_into()
    .map_err(|_| CoreError::Malformed("item key"))?;
    let plaintext = aead_open(
        &item_key,
        &envelope.body,
        &aad(BODY_LABEL, &group.group_id, &envelope.id),
    );
    zeroize::Zeroize::zeroize(&mut item_key);
    serde_json::from_slice(&plaintext?).map_err(|_| CoreError::Malformed("payload"))
}
