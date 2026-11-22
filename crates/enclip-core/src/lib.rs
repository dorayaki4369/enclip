//! enclip のプロトコル中核。
//!
//! 仕様の正本は `docs/crypto/key-protocol.md`。
//! 通信、保存、OS との連携は持たず、暗号、ペアリングの手順、アイテムの暗号化と受け入れ判定だけを扱う。
//! 状態はすべて呼び出し側が保存できる値として返す。

mod encoding;
mod error;
mod group;
mod item;
mod keys;
mod pairing;
mod primitives;
mod receive;

pub use encoding::{decode_b64, encode_b64};
pub use error::CoreError;
pub use group::{AccessToken, GroupId, GroupSecrets, UserKey};
pub use item::{ItemEnvelope, ItemId, ItemPayload, seal_item};
pub use keys::{DeviceKey, DevicePublic};
pub use pairing::{
    Challenge, Inviter, JoinRequest, Joiner, PairingId, QrPayload, SasCode, Welcome,
};
pub use receive::{Accepted, ReceiveState};
