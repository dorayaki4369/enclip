//! 暗号プリミティブの薄い包み。仕様 5 章のアルゴリズムだけを使う。

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hmac::{Hmac, Mac};
use hpke::aead::AesGcm256;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem, OpModeR, OpModeS, Serializable};
use sha2::Sha256;

use crate::CoreError;

pub(crate) type HpkeKem = X25519HkdfSha256;
pub(crate) type HpkePrivateKey = <HpkeKem as Kem>::PrivateKey;
pub(crate) type HpkePublicKey = <HpkeKem as Kem>::PublicKey;

const AES_GCM_NONCE_LEN: usize = 12;

/// OS の暗号論的乱数で埋めた固定長配列を返す。
pub(crate) fn random_array<const N: usize>() -> Result<[u8; N], CoreError> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|_| CoreError::Random)?;
    Ok(bytes)
}

/// 複数の部分をつないだ入力に対する HMAC-SHA256。
pub(crate) fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// HMAC-SHA256 を定数時間で検証する。
pub(crate) fn verify_hmac_sha256(key: &[u8], parts: &[&[u8]], tag: &[u8]) -> Result<(), CoreError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    for part in parts {
        mac.update(part);
    }
    mac.verify_slice(tag)
        .map_err(|_| CoreError::VerificationFailed)
}

/// HKDF-SHA256 で固定長の鍵を導出する。
pub(crate) fn hkdf_sha256<const N: usize>(ikm: &[u8], info: &[u8]) -> [u8; N] {
    let mut okm = [0u8; N];
    hkdf::Hkdf::<Sha256>::new(None, ikm)
        .expand(info, &mut okm)
        .expect("output length is within HKDF limits");
    okm
}

/// AES-256-GCM で暗号化し、`ノンス || 暗号文` を返す。ノンスは乱数で作る。
pub(crate) fn aead_seal(
    key: &[u8; 32],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, CoreError> {
    let nonce: [u8; AES_GCM_NONCE_LEN] = random_array()?;
    let cipher = Aes256Gcm::new(key.into());
    let ciphertext = cipher
        .encrypt(
            &Nonce::from(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| CoreError::VerificationFailed)?;
    let mut sealed = Vec::with_capacity(AES_GCM_NONCE_LEN + ciphertext.len());
    sealed.extend_from_slice(&nonce);
    sealed.extend_from_slice(&ciphertext);
    Ok(sealed)
}

/// `aead_seal` の出力を復号する。
pub(crate) fn aead_open(key: &[u8; 32], sealed: &[u8], aad: &[u8]) -> Result<Vec<u8>, CoreError> {
    if sealed.len() < AES_GCM_NONCE_LEN {
        return Err(CoreError::Malformed("aead"));
    }
    let (nonce, ciphertext) = sealed.split_at(AES_GCM_NONCE_LEN);
    let nonce: [u8; AES_GCM_NONCE_LEN] = nonce.try_into().expect("split at nonce length");
    let cipher = Aes256Gcm::new(key.into());
    cipher
        .decrypt(
            &Nonce::from(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| CoreError::VerificationFailed)
}

/// HPKE（RFC 9180、DHKEM(X25519, HKDF-SHA256)、HKDF-SHA256、AES-256-GCM）の base モードで封をする。
/// 戻り値はカプセル化した鍵と暗号文。
pub(crate) fn hpke_seal(
    recipient: &HpkePublicKey,
    info: &[u8],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<([u8; 32], Vec<u8>), CoreError> {
    let (encapped, ciphertext) = hpke::single_shot_seal::<AesGcm256, HkdfSha256, HpkeKem>(
        &OpModeS::Base,
        recipient,
        info,
        plaintext,
        aad,
    )
    .map_err(|_| CoreError::VerificationFailed)?;
    let encapped: [u8; 32] = encapped
        .to_bytes()
        .as_slice()
        .try_into()
        .expect("X25519 encapsulated key is 32 bytes");
    Ok((encapped, ciphertext))
}

pub(crate) fn hpke_open(
    private_key: &HpkePrivateKey,
    encapped: &[u8; 32],
    info: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, CoreError> {
    let encapped = <HpkeKem as Kem>::EncappedKey::from_bytes(encapped)
        .map_err(|_| CoreError::Malformed("hpke encapsulated key"))?;
    hpke::single_shot_open::<AesGcm256, HkdfSha256, HpkeKem>(
        &OpModeR::Base,
        private_key,
        &encapped,
        info,
        ciphertext,
        aad,
    )
    .map_err(|_| CoreError::VerificationFailed)
}

pub(crate) fn hpke_public_from_bytes(bytes: &[u8; 32]) -> Result<HpkePublicKey, CoreError> {
    HpkePublicKey::from_bytes(bytes).map_err(|_| CoreError::Malformed("hpke public key"))
}

pub(crate) fn hpke_public_to_bytes(key: &HpkePublicKey) -> [u8; 32] {
    key.to_bytes()
        .as_slice()
        .try_into()
        .expect("X25519 public key is 32 bytes")
}
