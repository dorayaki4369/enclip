//! iOS（Swift）から enclip-core を使うための橋渡し（UniFFI）。
//!
//! iPhone は PoC では新端末として参加し、アイテムを送受信する。
//! 鍵やメッセージはバイト列か JSON 文字列で受け渡し、通信と保存は Swift 側が持つ。

use std::sync::Mutex;

use enclip_core::{
    AccessToken, Challenge, CoreError, DeviceKey, GroupId, GroupSecrets, ItemEnvelope, ItemPayload,
    Joiner, ReceiveState, UserKey, Welcome, seal_item,
};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum EnclipError {
    #[error("malformed input")]
    Malformed,
    #[error("verification failed")]
    VerificationFailed,
    #[error("unexpected step")]
    UnexpectedStep,
    #[error("item expired")]
    Expired,
    #[error("duplicate item")]
    Duplicate,
    #[error("random source unavailable")]
    Random,
}

impl From<CoreError> for EnclipError {
    fn from(error: CoreError) -> Self {
        match error {
            CoreError::Malformed(_) => Self::Malformed,
            CoreError::VerificationFailed => Self::VerificationFailed,
            CoreError::UnexpectedStep(_) => Self::UnexpectedStep,
            CoreError::Expired => Self::Expired,
            CoreError::Duplicate => Self::Duplicate,
            CoreError::Random => Self::Random,
        }
    }
}

type Result<T> = std::result::Result<T, EnclipError>;

/// 端末が保存するグループの秘密。キーチェーンに保存する。
#[derive(uniffi::Record)]
pub struct GroupRecord {
    pub group_id: Vec<u8>,
    pub user_key: Vec<u8>,
    pub access_token: Vec<u8>,
}

impl GroupRecord {
    fn to_secrets(&self) -> Result<GroupSecrets> {
        Ok(GroupSecrets {
            group_id: GroupId(array(&self.group_id)?),
            user_key: UserKey(array(&self.user_key)?),
            access_token: AccessToken(array(&self.access_token)?),
        })
    }

    fn from_secrets(secrets: &GroupSecrets) -> Self {
        Self {
            group_id: secrets.group_id.0.to_vec(),
            user_key: secrets.user_key.0.to_vec(),
            access_token: secrets.access_token.0.to_vec(),
        }
    }
}

/// 受け入れたアイテムの中身。
#[derive(uniffi::Record)]
pub struct ItemRecord {
    pub id: String,
    pub text: String,
    pub sensitive: bool,
    pub from_self: bool,
    pub created_ms: u64,
    pub expires_ms: u64,
}

/// 受け入れの結果。`state_json` は受け入れの成否にかかわらず保存し直す。
#[derive(uniffi::Record)]
pub struct AcceptOutcome {
    pub state_json: String,
    pub item: Option<ItemRecord>,
}

fn array<const N: usize>(bytes: &[u8]) -> Result<[u8; N]> {
    bytes.try_into().map_err(|_| EnclipError::Malformed)
}

fn device(device_key: &[u8]) -> Result<DeviceKey> {
    Ok(DeviceKey::from_bytes(array(device_key)?))
}

#[uniffi::export]
pub fn generate_device_key() -> Result<Vec<u8>> {
    Ok(DeviceKey::generate()?.to_bytes().to_vec())
}

#[uniffi::export]
pub fn encode_base64url(bytes: Vec<u8>) -> String {
    enclip_core::encode_b64(&bytes)
}

/// 新端末側のペアリング（仕様 4.1）。
#[derive(uniffi::Object)]
pub struct JoinFlow {
    joiner: Mutex<Joiner>,
    join_request_json: String,
    group_id: Vec<u8>,
    pairing_id: Vec<u8>,
}

#[uniffi::export]
impl JoinFlow {
    /// QR コードを読み、参加申請を作る。
    #[uniffi::constructor]
    pub fn scan(device_key: Vec<u8>, qr_text: String) -> Result<Self> {
        let (joiner, request) = Joiner::scan(device(&device_key)?, &qr_text)?;
        let join_request_json =
            serde_json::to_string(&request).map_err(|_| EnclipError::Malformed)?;
        let group_id = joiner.qr().group_id.0.to_vec();
        let pairing_id = joiner.qr().pairing_id.0.to_vec();
        Ok(Self {
            joiner: Mutex::new(joiner),
            join_request_json,
            group_id,
            pairing_id,
        })
    }

    pub fn join_request_json(&self) -> String {
        self.join_request_json.clone()
    }

    pub fn group_id(&self) -> Vec<u8> {
        self.group_id.clone()
    }

    pub fn pairing_id(&self) -> Vec<u8> {
        self.pairing_id.clone()
    }

    /// 承認用ノンスを検証し、表示する 6 桁の確認コードを返す。
    pub fn receive_challenge(&self, challenge_json: String) -> Result<String> {
        let challenge: Challenge =
            serde_json::from_str(&challenge_json).map_err(|_| EnclipError::Malformed)?;
        let sas = self
            .joiner
            .lock()
            .expect("lock is not poisoned")
            .receive_challenge(&challenge)?;
        Ok(sas.0)
    }

    /// 既存端末の署名を検証し、グループの秘密を受け取る。
    pub fn receive_welcome(&self, welcome_json: String) -> Result<GroupRecord> {
        let welcome: Welcome =
            serde_json::from_str(&welcome_json).map_err(|_| EnclipError::Malformed)?;
        let secrets = self
            .joiner
            .lock()
            .expect("lock is not poisoned")
            .receive_welcome(&welcome)?;
        Ok(GroupRecord::from_secrets(&secrets))
    }
}

/// テキストを暗号化し、サーバーへ送る JSON を返す。
#[uniffi::export]
pub fn seal_text(
    group: GroupRecord,
    device_key: Vec<u8>,
    text: String,
    sensitive: bool,
    now_ms: u64,
    ttl_ms: u64,
) -> Result<String> {
    let payload = ItemPayload {
        text,
        sensitive,
        sender: device(&device_key)?.public().device_id(),
        created_ms: now_ms,
        expires_ms: now_ms.saturating_add(ttl_ms),
    };
    let envelope = seal_item(&group.to_secrets()?, &payload)?;
    serde_json::to_string(&envelope).map_err(|_| EnclipError::Malformed)
}

/// サーバーから受け取ったアイテムを検証して受け入れる。
/// 検証に失敗したもの、期限切れ、受け入れ済みは `item` が空になる。
#[uniffi::export]
pub fn accept_item(
    group: GroupRecord,
    device_key: Vec<u8>,
    state_json: Option<String>,
    envelope_json: String,
    now_ms: u64,
) -> Result<AcceptOutcome> {
    let mut state = match state_json {
        Some(json) => ReceiveState::from_json(&json)?,
        None => ReceiveState::default(),
    };
    let envelope: ItemEnvelope =
        serde_json::from_str(&envelope_json).map_err(|_| EnclipError::Malformed)?;
    let my_id = device(&device_key)?.public().device_id();
    let item = state
        .accept(&group.to_secrets()?, &envelope, now_ms)
        .ok()
        .map(|accepted| ItemRecord {
            id: enclip_core::encode_b64(&accepted.id.0),
            from_self: accepted.payload.sender == my_id,
            text: accepted.payload.text,
            sensitive: accepted.payload.sensitive,
            created_ms: accepted.payload.created_ms,
            expires_ms: accepted.payload.expires_ms,
        });
    Ok(AcceptOutcome {
        state_json: state.to_json(),
        item,
    })
}
