use thiserror::Error;

/// 中核が返すエラー。
///
/// 攻撃者に手がかりを与えないよう、検証の失敗は理由を細かく分けない。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// 形式が不正な入力（長さ、符号化、JSON）。
    #[error("malformed input: {0}")]
    Malformed(&'static str),
    /// MAC、署名、AEAD のいずれかの検証に失敗した。
    #[error("verification failed")]
    VerificationFailed,
    /// 手順の順序に合わない呼び出し。
    #[error("unexpected step: {0}")]
    UnexpectedStep(&'static str),
    /// アイテムの期限が切れている。
    #[error("item expired")]
    Expired,
    /// 受け入れ済みのアイテム。
    #[error("duplicate item")]
    Duplicate,
    /// OS の乱数源が使えない。
    #[error("random source unavailable")]
    Random,
}
