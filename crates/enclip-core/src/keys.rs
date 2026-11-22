//! 端末鍵（仕様 3 章）。
//!
//! 端末鍵は 32 バイトの乱数 1 つで表し、署名用の Ed25519 鍵と、ユーザー鍵を受け取るための
//! X25519 鍵（HPKE）をそこから導出する。用途ごとに鍵を分けるため、モデルの 1 つの鍵ペアを 2 つに分けている。

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hpke::Kem;
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::CoreError;
use crate::primitives::{
    HpkeKem, HpkePrivateKey, HpkePublicKey, hkdf_sha256, hpke_public_from_bytes,
    hpke_public_to_bytes, random_array,
};

const SIGN_KEY_INFO: &[u8] = b"enclip/device/sign/v1";
const KEM_KEY_INFO: &[u8] = b"enclip/device/kem/v1";

/// 端末の秘密鍵。端末の安全な保存領域にだけ置き、端末の外に出さない。
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct DeviceKey {
    seed: [u8; 32],
}

impl DeviceKey {
    pub fn generate() -> Result<Self, CoreError> {
        Ok(Self {
            seed: random_array()?,
        })
    }

    /// 保存した `to_bytes` の値から復元する。
    pub fn from_bytes(seed: [u8; 32]) -> Self {
        Self { seed }
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.seed
    }

    pub fn public(&self) -> DevicePublic {
        DevicePublic {
            sign: self.signing_key().verifying_key().to_bytes(),
            kem: hpke_public_to_bytes(&self.kem_keypair().1),
        }
    }

    pub(crate) fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.signing_key().sign(message).to_bytes()
    }

    pub(crate) fn kem_private(&self) -> HpkePrivateKey {
        self.kem_keypair().0
    }

    fn signing_key(&self) -> SigningKey {
        SigningKey::from_bytes(&hkdf_sha256(&self.seed, SIGN_KEY_INFO))
    }

    fn kem_keypair(&self) -> (HpkePrivateKey, HpkePublicKey) {
        let ikm: [u8; 32] = hkdf_sha256(&self.seed, KEM_KEY_INFO);
        HpkeKem::derive_keypair(&ikm)
    }
}

/// 端末の公開鍵。署名用と鍵受け取り用の 2 つを持つ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevicePublic {
    pub sign: [u8; 32],
    pub kem: [u8; 32],
}

impl DevicePublic {
    pub const LEN: usize = 64;

    /// `署名用 || 鍵受け取り用` の 64 バイト。MAC や署名の対象にもこの形を使う。
    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        let mut bytes = [0u8; Self::LEN];
        bytes[..32].copy_from_slice(&self.sign);
        bytes[32..].copy_from_slice(&self.kem);
        bytes
    }

    pub fn from_bytes(bytes: &[u8; Self::LEN]) -> Self {
        let mut sign = [0u8; 32];
        let mut kem = [0u8; 32];
        sign.copy_from_slice(&bytes[..32]);
        kem.copy_from_slice(&bytes[32..]);
        Self { sign, kem }
    }

    /// 送信元の表示などに使う端末 ID（公開鍵の SHA-256 の先頭 16 バイト）。
    pub fn device_id(&self) -> [u8; 16] {
        let digest = Sha256::digest(self.to_bytes());
        digest[..16]
            .try_into()
            .expect("digest is longer than 16 bytes")
    }

    pub(crate) fn verify(
        sign_public: &[u8; 32],
        message: &[u8],
        signature: &[u8; 64],
    ) -> Result<(), CoreError> {
        let key =
            VerifyingKey::from_bytes(sign_public).map_err(|_| CoreError::VerificationFailed)?;
        key.verify_strict(message, &Signature::from_bytes(signature))
            .map_err(|_| CoreError::VerificationFailed)
    }

    pub(crate) fn kem_public(&self) -> Result<HpkePublicKey, CoreError> {
        hpke_public_from_bytes(&self.kem)
    }
}
