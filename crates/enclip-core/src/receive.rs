//! 受信の判定（仕様 4.3、4.4）。
//!
//! 期限の判定と記録の削除には、これまでに観測した最も新しい時刻を使う。
//! 時計が戻ったときに、記録を消した後の再送を受け入れないため（`docs/crypto/replay_check.py`）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::item::open_item;
use crate::{CoreError, GroupSecrets, ItemEnvelope, ItemId, ItemPayload};

/// 受信側の状態。呼び出し側が `to_json` で保存し、`from_json` で復元する。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiveState {
    /// 受け入れたアイテム ID と、その期限。
    accepted: BTreeMap<String, u64>,
    /// これまでに観測した最も新しい時刻（UNIX ミリ秒）。戻さない。
    high_water_ms: u64,
}

/// 受け入れたアイテム。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accepted {
    pub id: ItemId,
    pub payload: ItemPayload,
}

impl ReceiveState {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("state is serializable")
    }

    pub fn from_json(text: &str) -> Result<Self, CoreError> {
        serde_json::from_str(text).map_err(|_| CoreError::Malformed("receive state"))
    }

    /// 判定に使う現在時刻。時計が戻っても前に観測した時刻より前にはならない。
    pub fn effective_now(&self, clock_ms: u64) -> u64 {
        self.high_water_ms.max(clock_ms)
    }

    /// 時刻を進め、期限を過ぎた記録を消す。
    pub fn advance(&mut self, clock_ms: u64) {
        self.high_water_ms = self.effective_now(clock_ms);
        let now = self.high_water_ms;
        self.accepted.retain(|_, expires_ms| now < *expires_ms);
    }

    /// アイテムを復号し、期限内で未受信なら受け入れて記録する。
    pub fn accept(
        &mut self,
        group: &GroupSecrets,
        envelope: &ItemEnvelope,
        clock_ms: u64,
    ) -> Result<Accepted, CoreError> {
        self.advance(clock_ms);
        let key = crate::encode_b64(&envelope.id.0);
        if self.accepted.contains_key(&key) {
            return Err(CoreError::Duplicate);
        }
        let payload = open_item(group, envelope)?;
        if self.high_water_ms >= payload.expires_ms {
            return Err(CoreError::Expired);
        }
        self.accepted.insert(key, payload.expires_ms);
        Ok(Accepted {
            id: envelope.id,
            payload,
        })
    }
}
