use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::CoreError;

/// サーバーとの受け渡しや QR コードで使う base64url（パディングなし）。
pub fn encode_b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn decode_b64(text: &str) -> Result<Vec<u8>, CoreError> {
    URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| CoreError::Malformed("base64"))
}

/// 固定長の配列として復号する。
pub(crate) fn decode_b64_array<const N: usize>(text: &str) -> Result<[u8; N], CoreError> {
    decode_b64(text)?
        .try_into()
        .map_err(|_| CoreError::Malformed("length"))
}

/// 固定長バイト列を JSON で base64url 文字列として扱うための serde 補助。
pub(crate) mod b64_array {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer, const N: usize>(
        bytes: &[u8; N],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::encode_b64(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        deserializer: D,
    ) -> Result<[u8; N], D::Error> {
        let text = String::deserialize(deserializer)?;
        super::decode_b64_array(&text).map_err(D::Error::custom)
    }
}

/// 可変長バイト列を JSON で base64url 文字列として扱うための serde 補助。
pub(crate) mod b64_vec {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::encode_b64(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        super::decode_b64(&text).map_err(D::Error::custom)
    }
}
