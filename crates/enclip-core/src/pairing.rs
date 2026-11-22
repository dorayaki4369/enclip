//! QR コードによる端末の追加（仕様 4.1）。
//!
//! 既存端末（A）は `Inviter`、新端末（B）は `Joiner` として手順を進める。
//! どちらも 1 回のペアリングだけを扱い、順序に合わない呼び出しは `UnexpectedStep` を返す。

use serde::{Deserialize, Serialize};

use crate::encoding::{b64_array, b64_vec, decode_b64, encode_b64};
use crate::primitives::{hmac_sha256, hpke_open, hpke_seal, random_array, verify_hmac_sha256};
use crate::{AccessToken, CoreError, DeviceKey, DevicePublic, GroupId, GroupSecrets, UserKey};

const QR_PREFIX: &str = "ENCLIP1:";
const QR_VERSION: u8 = 1;
const JOIN_LABEL: &[u8] = b"enclip/join/v1";
const CHALLENGE_LABEL: &[u8] = b"enclip/challenge/v1";
const SAS_LABEL: &[u8] = b"enclip/sas/v1";
const WELCOME_LABEL: &[u8] = b"enclip/welcome/v1";

/// ペアリング ID。サーバーでのメッセージ受け渡しの宛先に使う。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PairingId(pub [u8; 16]);

/// QR コードに載せる内容（仕様 4.1 の 2）。
#[derive(Clone)]
pub struct QrPayload {
    pub pairing_id: PairingId,
    pairing_secret: [u8; 32],
    pub inviter_sign: [u8; 32],
    pub group_id: GroupId,
}

impl QrPayload {
    const BODY_LEN: usize = 1 + 16 + 32 + 32 + 16;

    pub fn encode(&self) -> String {
        let mut body = Vec::with_capacity(Self::BODY_LEN);
        body.push(QR_VERSION);
        body.extend_from_slice(&self.pairing_id.0);
        body.extend_from_slice(&self.pairing_secret);
        body.extend_from_slice(&self.inviter_sign);
        body.extend_from_slice(&self.group_id.0);
        format!("{QR_PREFIX}{}", encode_b64(&body))
    }

    pub fn decode(text: &str) -> Result<Self, CoreError> {
        let body = text
            .strip_prefix(QR_PREFIX)
            .ok_or(CoreError::Malformed("qr prefix"))?;
        let body = decode_b64(body)?;
        if body.len() != Self::BODY_LEN || body[0] != QR_VERSION {
            return Err(CoreError::Malformed("qr body"));
        }
        let field = |start: usize, len: usize| &body[start..start + len];
        Ok(Self {
            pairing_id: PairingId(field(1, 16).try_into().expect("fixed slice")),
            pairing_secret: field(17, 32).try_into().expect("fixed slice"),
            inviter_sign: field(49, 32).try_into().expect("fixed slice"),
            group_id: GroupId(field(81, 16).try_into().expect("fixed slice")),
        })
    }
}

/// 参加申請（仕様 4.1 の 3）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinRequest {
    #[serde(with = "b64_array")]
    pub joiner: [u8; DevicePublic::LEN],
    #[serde(with = "b64_array")]
    mac: [u8; 32],
}

/// 承認用ノンスと署名（仕様 4.1 の 4）。
/// 署名はペアリング ID、新端末の公開鍵、ノンスの 3 つを覆う。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Challenge {
    #[serde(with = "b64_array")]
    nonce: [u8; 32],
    #[serde(with = "b64_array")]
    signature: [u8; 64],
}

/// ユーザー鍵とアクセス用トークンを新端末の公開鍵で包んだもの（仕様 4.1 の 7）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    #[serde(with = "b64_array")]
    encapped: [u8; 32],
    #[serde(with = "b64_vec")]
    ciphertext: Vec<u8>,
    #[serde(with = "b64_array")]
    signature: [u8; 64],
}

/// 両画面に出す 6 桁の確認コード。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SasCode(pub String);

impl SasCode {
    fn compute(
        pairing_secret: &[u8; 32],
        joiner: &[u8; DevicePublic::LEN],
        nonce: &[u8; 32],
    ) -> Self {
        let digest = hmac_sha256(pairing_secret, &[SAS_LABEL, joiner, nonce]);
        let number = u32::from_be_bytes(digest[..4].try_into().expect("digest has 4 bytes"));
        Self(format!("{:06}", number % 1_000_000))
    }
}

fn join_mac_parts<'a>(
    pairing_id: &'a PairingId,
    joiner: &'a [u8; DevicePublic::LEN],
) -> [&'a [u8]; 3] {
    [JOIN_LABEL, &pairing_id.0, joiner]
}

fn challenge_message(
    pairing_id: &PairingId,
    joiner: &[u8; DevicePublic::LEN],
    nonce: &[u8; 32],
) -> Vec<u8> {
    [CHALLENGE_LABEL, &pairing_id.0, joiner, nonce].concat()
}

fn welcome_aad(pairing_id: &PairingId, group_id: &GroupId) -> Vec<u8> {
    [WELCOME_LABEL, &pairing_id.0, &group_id.0].concat()
}

fn welcome_message(
    pairing_id: &PairingId,
    joiner: &[u8; DevicePublic::LEN],
    encapped: &[u8; 32],
    ciphertext: &[u8],
) -> Vec<u8> {
    [WELCOME_LABEL, &pairing_id.0, joiner, encapped, ciphertext].concat()
}

/// 既存端末側のペアリング。
pub struct Inviter {
    device: DeviceKey,
    group: GroupSecrets,
    qr: QrPayload,
    state: InviterState,
}

enum InviterState {
    AwaitingJoin,
    AwaitingApproval { joiner: DevicePublic },
    Finished,
}

impl Inviter {
    /// ペアリング ID とペアリング秘密を作り、QR コードの内容を用意する（仕様 4.1 の 1、2）。
    pub fn start(device: DeviceKey, group: GroupSecrets) -> Result<Self, CoreError> {
        let qr = QrPayload {
            pairing_id: PairingId(random_array()?),
            pairing_secret: random_array()?,
            inviter_sign: device.public().sign,
            group_id: group.group_id,
        };
        Ok(Self {
            device,
            group,
            qr,
            state: InviterState::AwaitingJoin,
        })
    }

    pub fn qr(&self) -> &QrPayload {
        &self.qr
    }

    /// 参加申請の MAC を検証し、承認用ノンスを返す（仕様 4.1 の 4、5）。
    /// MAC が合わない申請は捨て、次の申請を待ち続ける。
    pub fn receive_join(
        &mut self,
        request: &JoinRequest,
    ) -> Result<(Challenge, SasCode), CoreError> {
        if !matches!(self.state, InviterState::AwaitingJoin) {
            return Err(CoreError::UnexpectedStep("join already received"));
        }
        verify_hmac_sha256(
            &self.qr.pairing_secret,
            &join_mac_parts(&self.qr.pairing_id, &request.joiner),
            &request.mac,
        )?;
        let joiner = DevicePublic::from_bytes(&request.joiner);
        joiner.kem_public()?;
        let nonce: [u8; 32] = random_array()?;
        let signature = self.device.sign(&challenge_message(
            &self.qr.pairing_id,
            &request.joiner,
            &nonce,
        ));
        let sas = SasCode::compute(&self.qr.pairing_secret, &request.joiner, &nonce);
        self.state = InviterState::AwaitingApproval { joiner };
        Ok((Challenge { nonce, signature }, sas))
    }

    /// 利用者が確認コードの一致を確かめて承認した後、ユーザー鍵を包んで返す（仕様 4.1 の 6、7）。
    pub fn approve(&mut self) -> Result<Welcome, CoreError> {
        let InviterState::AwaitingApproval { joiner } = self.state else {
            return Err(CoreError::UnexpectedStep("no join to approve"));
        };
        let mut plaintext = Vec::with_capacity(64);
        plaintext.extend_from_slice(&self.group.user_key.0);
        plaintext.extend_from_slice(&self.group.access_token.0);
        let (encapped, ciphertext) = hpke_seal(
            &joiner.kem_public()?,
            WELCOME_LABEL,
            &plaintext,
            &welcome_aad(&self.qr.pairing_id, &self.group.group_id),
        )?;
        zeroize::Zeroize::zeroize(&mut plaintext);
        let joiner_bytes = joiner.to_bytes();
        let signature = self.device.sign(&welcome_message(
            &self.qr.pairing_id,
            &joiner_bytes,
            &encapped,
            &ciphertext,
        ));
        self.state = InviterState::Finished;
        Ok(Welcome {
            encapped,
            ciphertext,
            signature,
        })
    }
}

/// 新端末側のペアリング。
pub struct Joiner {
    device: DeviceKey,
    qr: QrPayload,
    state: JoinerState,
}

enum JoinerState {
    AwaitingChallenge,
    AwaitingWelcome,
    Finished,
}

impl Joiner {
    /// QR コードを読み、参加申請を作る（仕様 4.1 の 3）。
    pub fn scan(device: DeviceKey, qr_text: &str) -> Result<(Self, JoinRequest), CoreError> {
        let qr = QrPayload::decode(qr_text)?;
        let joiner = device.public().to_bytes();
        let mac = hmac_sha256(&qr.pairing_secret, &join_mac_parts(&qr.pairing_id, &joiner));
        Ok((
            Self {
                device,
                qr,
                state: JoinerState::AwaitingChallenge,
            },
            JoinRequest { joiner, mac },
        ))
    }

    pub fn qr(&self) -> &QrPayload {
        &self.qr
    }

    /// 承認用ノンスの署名を検証し、確認コードを返す（仕様 4.1 の 5）。
    /// 署名された公開鍵が自分のものでなければ、確認コードを返さない。
    pub fn receive_challenge(&mut self, challenge: &Challenge) -> Result<SasCode, CoreError> {
        if !matches!(self.state, JoinerState::AwaitingChallenge) {
            return Err(CoreError::UnexpectedStep("challenge already received"));
        }
        let joiner = self.device.public().to_bytes();
        DevicePublic::verify(
            &self.qr.inviter_sign,
            &challenge_message(&self.qr.pairing_id, &joiner, &challenge.nonce),
            &challenge.signature,
        )?;
        self.state = JoinerState::AwaitingWelcome;
        Ok(SasCode::compute(
            &self.qr.pairing_secret,
            &joiner,
            &challenge.nonce,
        ))
    }

    /// 既存端末の署名を検証してから、ユーザー鍵とアクセス用トークンを取り出す（仕様 4.1 の 8）。
    pub fn receive_welcome(&mut self, welcome: &Welcome) -> Result<GroupSecrets, CoreError> {
        if !matches!(self.state, JoinerState::AwaitingWelcome) {
            return Err(CoreError::UnexpectedStep("welcome before challenge"));
        }
        let joiner = self.device.public().to_bytes();
        DevicePublic::verify(
            &self.qr.inviter_sign,
            &welcome_message(
                &self.qr.pairing_id,
                &joiner,
                &welcome.encapped,
                &welcome.ciphertext,
            ),
            &welcome.signature,
        )?;
        let mut plaintext = hpke_open(
            &self.device.kem_private(),
            &welcome.encapped,
            WELCOME_LABEL,
            &welcome.ciphertext,
            &welcome_aad(&self.qr.pairing_id, &self.qr.group_id),
        )?;
        if plaintext.len() != 64 {
            zeroize::Zeroize::zeroize(&mut plaintext);
            return Err(CoreError::Malformed("welcome payload"));
        }
        let secrets = GroupSecrets {
            group_id: self.qr.group_id,
            user_key: UserKey(plaintext[..32].try_into().expect("fixed slice")),
            access_token: AccessToken(plaintext[32..].try_into().expect("fixed slice")),
        };
        zeroize::Zeroize::zeroize(&mut plaintext);
        self.state = JoinerState::Finished;
        Ok(secrets)
    }
}
