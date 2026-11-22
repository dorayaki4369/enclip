//! グループの秘密（仕様 3 章）。

use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::CoreError;
use crate::primitives::random_array;

/// グループ ID。最初の端末が乱数で作る。サーバーへの宛先にも使うので秘密ではない。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GroupId(pub [u8; 16]);

/// ユーザー鍵。アイテム鍵を包む。
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct UserKey(pub [u8; 32]);

/// サーバーへのアクセス用トークン。ユーザー鍵と一緒に暗号化して新端末へ渡す。
/// QR コードには載らないので、QR コードを盗み見てもサーバーを使えない。
/// 内容の秘匿と真正性は暗号が担い、このトークンは配信の妨害を防ぐためだけに使う。
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct AccessToken(pub [u8; 32]);

/// 端末が保存するグループの情報一式。
#[derive(Clone)]
pub struct GroupSecrets {
    pub group_id: GroupId,
    pub user_key: UserKey,
    pub access_token: AccessToken,
}

impl GroupSecrets {
    /// 最初の端末がグループを作る。
    pub fn create() -> Result<Self, CoreError> {
        Ok(Self {
            group_id: GroupId(random_array()?),
            user_key: UserKey(random_array()?),
            access_token: AccessToken(random_array()?),
        })
    }
}
