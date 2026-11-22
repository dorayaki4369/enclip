//! 鍵プロトコルの性質を、形式検証の補題と反例に対応させて確かめる。

use enclip_core::{
    CoreError, DeviceKey, GroupSecrets, Inviter, ItemEnvelope, ItemPayload, Joiner, ReceiveState,
    seal_item,
};

const HOUR_MS: u64 = 60 * 60 * 1000;

struct Paired {
    inviter_group: GroupSecrets,
    joiner_group: GroupSecrets,
}

fn pair() -> Paired {
    let group = GroupSecrets::create().unwrap();
    let mut inviter = Inviter::start(DeviceKey::generate().unwrap(), group.clone()).unwrap();
    let (mut joiner, request) =
        Joiner::scan(DeviceKey::generate().unwrap(), &inviter.qr().encode()).unwrap();
    let (challenge, inviter_sas) = inviter.receive_join(&request).unwrap();
    let joiner_sas = joiner.receive_challenge(&challenge).unwrap();
    assert_eq!(inviter_sas, joiner_sas);
    let welcome = inviter.approve().unwrap();
    let joiner_group = joiner.receive_welcome(&welcome).unwrap();
    Paired {
        inviter_group: group,
        joiner_group,
    }
}

fn payload(text: &str, created_ms: u64) -> ItemPayload {
    ItemPayload {
        text: text.to_owned(),
        sensitive: false,
        sender: [7; 16],
        created_ms,
        expires_ms: created_ms + HOUR_MS,
    }
}

/// 端末間の受け渡しを JSON で往復させる（サーバー経由を模す）。
fn via_server<T: serde::Serialize + serde::de::DeserializeOwned>(message: &T) -> T {
    serde_json::from_str(&serde_json::to_string(message).unwrap()).unwrap()
}

#[test]
fn paired_devices_share_group_and_exchange_items() {
    let paired = pair();
    assert_eq!(paired.inviter_group.group_id, paired.joiner_group.group_id);
    assert_eq!(
        paired.inviter_group.user_key.0,
        paired.joiner_group.user_key.0
    );
    assert_eq!(
        paired.inviter_group.access_token.0,
        paired.joiner_group.access_token.0
    );

    let sent = payload("hello", 1_000);
    let envelope = via_server(&seal_item(&paired.inviter_group, &sent).unwrap());
    let mut state = ReceiveState::default();
    let accepted = state
        .accept(&paired.joiner_group, &envelope, 2_000)
        .unwrap();
    assert_eq!(accepted.payload, sent);
}

/// `approve_only_scanned_device` と `-D=WEAKSIG` の反例に対応する。
/// QR コードを盗み見た攻撃者が先に申請しても、正規の新端末は確認コードを表示しない。
#[test]
fn leaked_qr_attacker_cannot_make_joiner_show_a_code() {
    let group = GroupSecrets::create().unwrap();
    let mut inviter = Inviter::start(DeviceKey::generate().unwrap(), group).unwrap();
    let qr = inviter.qr().encode();
    let (_attacker, attacker_request) = Joiner::scan(DeviceKey::generate().unwrap(), &qr).unwrap();
    let (mut honest, _honest_request) = Joiner::scan(DeviceKey::generate().unwrap(), &qr).unwrap();

    let (challenge_for_attacker, _) = inviter.receive_join(&attacker_request).unwrap();
    assert_eq!(
        honest.receive_challenge(&challenge_for_attacker),
        Err(CoreError::VerificationFailed)
    );
}

#[test]
fn join_request_with_wrong_pairing_secret_is_rejected() {
    let group = GroupSecrets::create().unwrap();
    let mut inviter = Inviter::start(DeviceKey::generate().unwrap(), group.clone()).unwrap();
    let other = Inviter::start(DeviceKey::generate().unwrap(), group).unwrap();
    let (_joiner, request) =
        Joiner::scan(DeviceKey::generate().unwrap(), &other.qr().encode()).unwrap();
    assert_eq!(
        inviter.receive_join(&request),
        Err(CoreError::VerificationFailed)
    );
    // 中核は、不正な申請の後も正しい申請を受け付ける。
    // サーバーの参加申請欄は最初の 1 件しか書けないので、実際には利用者がペアリングをやり直す。
    let (_joiner, request) =
        Joiner::scan(DeviceKey::generate().unwrap(), &inviter.qr().encode()).unwrap();
    assert!(inviter.receive_join(&request).is_ok());
}

#[test]
fn one_pairing_approves_only_one_device() {
    let group = GroupSecrets::create().unwrap();
    let mut inviter = Inviter::start(DeviceKey::generate().unwrap(), group).unwrap();
    let qr = inviter.qr().encode();
    let (_a, first) = Joiner::scan(DeviceKey::generate().unwrap(), &qr).unwrap();
    let (_b, second) = Joiner::scan(DeviceKey::generate().unwrap(), &qr).unwrap();
    inviter.receive_join(&first).unwrap();
    assert!(matches!(
        inviter.receive_join(&second),
        Err(CoreError::UnexpectedStep(_))
    ));
    inviter.approve().unwrap();
    assert!(matches!(
        inviter.approve(),
        Err(CoreError::UnexpectedStep(_))
    ));
}

#[test]
fn approve_requires_a_join_request() {
    let group = GroupSecrets::create().unwrap();
    let mut inviter = Inviter::start(DeviceKey::generate().unwrap(), group).unwrap();
    assert!(matches!(
        inviter.approve(),
        Err(CoreError::UnexpectedStep(_))
    ));
}

/// `joined_key_is_genuine` に対応する。別の端末が署名した welcome は受け入れない。
#[test]
fn welcome_signed_by_another_device_is_rejected() {
    let group = GroupSecrets::create().unwrap();
    let mut inviter = Inviter::start(DeviceKey::generate().unwrap(), group.clone()).unwrap();
    let (mut joiner, request) =
        Joiner::scan(DeviceKey::generate().unwrap(), &inviter.qr().encode()).unwrap();
    let (challenge, _) = inviter.receive_join(&request).unwrap();
    joiner.receive_challenge(&challenge).unwrap();

    // 攻撃者は別の鍵で同じ新端末あての welcome を作れるが、QR の公開鍵では検証できない。
    let mut impostor = Inviter::start(DeviceKey::generate().unwrap(), group).unwrap();
    let (_, impostor_request) =
        Joiner::scan(DeviceKey::from_bytes([1; 32]), &impostor.qr().encode()).unwrap();
    impostor.receive_join(&impostor_request).unwrap();
    let forged = impostor.approve().unwrap();
    assert_eq!(
        joiner.receive_welcome(&forged).err(),
        Some(CoreError::VerificationFailed)
    );
}

#[test]
fn welcome_before_challenge_is_rejected() {
    let group = GroupSecrets::create().unwrap();
    let mut inviter = Inviter::start(DeviceKey::generate().unwrap(), group).unwrap();
    let (mut joiner, request) =
        Joiner::scan(DeviceKey::generate().unwrap(), &inviter.qr().encode()).unwrap();
    inviter.receive_join(&request).unwrap();
    let welcome = inviter.approve().unwrap();
    assert!(matches!(
        joiner.receive_welcome(&welcome),
        Err(CoreError::UnexpectedStep(_))
    ));
}

#[test]
fn tampered_welcome_is_rejected() {
    let group = GroupSecrets::create().unwrap();
    let mut inviter = Inviter::start(DeviceKey::generate().unwrap(), group).unwrap();
    let (mut joiner, request) =
        Joiner::scan(DeviceKey::generate().unwrap(), &inviter.qr().encode()).unwrap();
    let (challenge, _) = inviter.receive_join(&request).unwrap();
    joiner.receive_challenge(&challenge).unwrap();
    let welcome = inviter.approve().unwrap();

    let mut json: serde_json::Value = serde_json::to_value(&welcome).unwrap();
    let ciphertext = json["ciphertext"].as_str().unwrap().to_owned();
    let mut bytes = enclip_core::decode_b64(&ciphertext).unwrap();
    bytes[0] ^= 1;
    json["ciphertext"] = enclip_core::encode_b64(&bytes).into();
    let tampered = serde_json::from_value(json).unwrap();
    assert_eq!(
        joiner.receive_welcome(&tampered).err(),
        Some(CoreError::VerificationFailed)
    );
}

/// `item_injective_agreement` と `-D=DEDUP` の反例に対応する。
#[test]
fn replayed_item_is_rejected() {
    let paired = pair();
    let envelope = seal_item(&paired.inviter_group, &payload("once", 1_000)).unwrap();
    let mut state = ReceiveState::default();
    state
        .accept(&paired.joiner_group, &envelope, 2_000)
        .unwrap();
    assert_eq!(
        state.accept(&paired.joiner_group, &envelope, 3_000),
        Err(CoreError::Duplicate)
    );
}

/// `replay_check.py` の反例（時計の巻き戻し）に対応する。
#[test]
fn replay_after_clock_rollback_is_rejected() {
    let paired = pair();
    let envelope = seal_item(&paired.inviter_group, &payload("once", 1_000)).unwrap();
    let mut state = ReceiveState::default();
    state
        .accept(&paired.joiner_group, &envelope, 2_000)
        .unwrap();
    // 期限を過ぎて記録が消える。
    state.advance(1_000 + HOUR_MS + 1);
    // 時計が戻っても、観測した最も新しい時刻で判定する。
    let restored = ReceiveState::from_json(&state.to_json()).unwrap();
    let mut state = restored;
    assert_eq!(
        state.accept(&paired.joiner_group, &envelope, 2_000),
        Err(CoreError::Expired)
    );
}

#[test]
fn expired_item_is_rejected() {
    let paired = pair();
    let envelope = seal_item(&paired.inviter_group, &payload("late", 1_000)).unwrap();
    let mut state = ReceiveState::default();
    assert_eq!(
        state.accept(&paired.joiner_group, &envelope, 1_000 + HOUR_MS),
        Err(CoreError::Expired)
    );
}

/// 履歴の読み込みは受け入れ済みのアイテムも読めるが、受け入れの記録は変えない。
#[test]
fn history_reads_accepted_items_without_recording() {
    let paired = pair();
    let seen = seal_item(&paired.inviter_group, &payload("seen", 1_000)).unwrap();
    let fresh = seal_item(&paired.inviter_group, &payload("fresh", 1_000)).unwrap();
    let mut state = ReceiveState::default();
    state.accept(&paired.joiner_group, &seen, 2_000).unwrap();

    let read = state
        .read_for_history(&paired.joiner_group, &seen, 2_000)
        .unwrap();
    assert_eq!(read.payload.text, "seen");
    state
        .read_for_history(&paired.joiner_group, &fresh, 2_000)
        .unwrap();
    // 読んだだけのアイテムは、新着として受け入れられる。
    assert!(state.accept(&paired.joiner_group, &fresh, 2_000).is_ok());
}

#[test]
fn history_skips_expired_and_foreign_items() {
    let paired = pair();
    let other = pair();
    let envelope = seal_item(&paired.inviter_group, &payload("late", 1_000)).unwrap();
    let state = ReceiveState::default();
    assert_eq!(
        state
            .read_for_history(&paired.joiner_group, &envelope, 1_000 + HOUR_MS)
            .map(|_| ()),
        Err(CoreError::Expired)
    );
    assert!(
        state
            .read_for_history(&other.joiner_group, &envelope, 2_000)
            .is_err()
    );
}

/// `item_secrecy` と関連データの結び付けに対応する。
#[test]
fn item_bound_to_group_and_id() {
    let paired = pair();
    let envelope = seal_item(&paired.inviter_group, &payload("bound", 1_000)).unwrap();

    // 別のグループの鍵では読めない。
    let outsider = GroupSecrets::create().unwrap();
    assert_eq!(
        ReceiveState::default().accept(&outsider, &envelope, 2_000),
        Err(CoreError::VerificationFailed)
    );

    // サーバーがアイテム ID を付け替えると検証に失敗する。
    let other = seal_item(&paired.inviter_group, &payload("other", 1_000)).unwrap();
    let swapped = ItemEnvelope {
        id: other.id,
        ..envelope.clone()
    };
    assert_eq!(
        ReceiveState::default().accept(&paired.joiner_group, &swapped, 2_000),
        Err(CoreError::VerificationFailed)
    );

    // 内容を改ざんすると検証に失敗する。
    let mut tampered = envelope.clone();
    let last = tampered.body.len() - 1;
    tampered.body[last] ^= 1;
    assert_eq!(
        ReceiveState::default().accept(&paired.joiner_group, &tampered, 2_000),
        Err(CoreError::VerificationFailed)
    );
}

#[test]
fn malformed_qr_is_rejected() {
    assert!(Joiner::scan(DeviceKey::generate().unwrap(), "ENCLIP1:AAAA").is_err());
    assert!(Joiner::scan(DeviceKey::generate().unwrap(), "https://example.com").is_err());
}

#[test]
fn device_key_roundtrip_keeps_public_key() {
    let key = DeviceKey::generate().unwrap();
    let restored = DeviceKey::from_bytes(key.to_bytes());
    assert_eq!(key.public(), restored.public());
}
