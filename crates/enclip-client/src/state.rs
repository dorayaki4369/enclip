//! 端末が保存する状態。保存先と保護（DPAPI、キーチェーンなど）は各アプリが決める。

use enclip_core::{
    AccessToken, CoreError, DeviceKey, GroupId, GroupSecrets, ReceiveState, UserKey, decode_b64,
    encode_b64,
};
use serde::{Deserialize, Serialize};

/// 端末の状態一式。秘密を含むので、保存時は各 OS の保護を使う。
#[derive(Clone, Serialize, Deserialize)]
pub struct DeviceState {
    device_key: String,
    pub group: Option<GroupState>,
}

/// 参加中のグループの状態。
#[derive(Clone, Serialize, Deserialize)]
pub struct GroupState {
    group_id: String,
    user_key: String,
    access_token: String,
    pub receive: ReceiveState,
    /// サーバーから取得済みの連番。
    pub cursor: u64,
}

impl DeviceState {
    pub fn generate() -> Result<Self, CoreError> {
        Ok(Self {
            device_key: encode_b64(&DeviceKey::generate()?.to_bytes()),
            group: None,
        })
    }

    pub fn device_key(&self) -> Result<DeviceKey, CoreError> {
        Ok(DeviceKey::from_bytes(decode_array(&self.device_key)?))
    }
}

impl GroupState {
    pub fn new(secrets: &GroupSecrets) -> Self {
        Self {
            group_id: encode_b64(&secrets.group_id.0),
            user_key: encode_b64(&secrets.user_key.0),
            access_token: encode_b64(&secrets.access_token.0),
            receive: ReceiveState::default(),
            cursor: 0,
        }
    }

    pub fn secrets(&self) -> Result<GroupSecrets, CoreError> {
        Ok(GroupSecrets {
            group_id: GroupId(decode_array(&self.group_id)?),
            user_key: UserKey(decode_array(&self.user_key)?),
            access_token: AccessToken(decode_array(&self.access_token)?),
        })
    }
}

fn decode_array<const N: usize>(text: &str) -> Result<[u8; N], CoreError> {
    decode_b64(text)?
        .try_into()
        .map_err(|_| CoreError::Malformed("stored key length"))
}
