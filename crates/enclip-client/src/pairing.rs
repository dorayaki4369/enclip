//! ペアリングの手順をサーバー経由で進める（仕様 4.1）。
//!
//! 待つ処理はサーバーを一定間隔で問い合わせる。打ち切りは呼び出し側が future を捨てて行う。

use std::time::Duration;

use enclip_core::{
    Challenge, CoreError, DeviceKey, GroupSecrets, Inviter, JoinRequest, Joiner, SasCode, Welcome,
};
use thiserror::Error;

use crate::api::{ApiError, GroupAuth, ServerClient};

const POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Error)]
pub enum PairingError {
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error(transparent)]
    Protocol(#[from] CoreError),
}

/// 既存端末側。QR コードを表示し、参加申請を受けて承認する。
pub struct InviteSession {
    client: ServerClient,
    auth: GroupAuth,
    inviter: Inviter,
}

impl InviteSession {
    pub async fn start(
        client: ServerClient,
        device: DeviceKey,
        group: GroupSecrets,
    ) -> Result<Self, PairingError> {
        let auth = GroupAuth::from_secrets(&group);
        let inviter = Inviter::start(device, group)?;
        client
            .create_pairing(&auth, &inviter.qr().pairing_id)
            .await?;
        Ok(Self {
            client,
            auth,
            inviter,
        })
    }

    pub fn qr_text(&self) -> String {
        self.inviter.qr().encode()
    }

    /// 参加申請を待ち、承認用ノンスを返して確認コードを得る。
    pub async fn wait_for_join(&mut self) -> Result<SasCode, PairingError> {
        let pairing = self.inviter.qr().pairing_id;
        let request: JoinRequest = loop {
            if let Some(request) = self.client.get_slot(&self.auth, &pairing, "join").await? {
                break request;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        };
        let (challenge, sas) = self.inviter.receive_join(&request)?;
        self.client
            .put_slot(&self.auth, &pairing, "challenge", &challenge)
            .await?;
        Ok(sas)
    }

    /// 利用者が確認コードの一致を確かめた後に呼ぶ。
    pub async fn approve(&mut self) -> Result<(), PairingError> {
        let welcome = self.inviter.approve()?;
        self.client
            .put_slot(
                &self.auth,
                &self.inviter.qr().pairing_id,
                "welcome",
                &welcome,
            )
            .await?;
        Ok(())
    }

    /// ペアリング画面を閉じたときに呼び、サーバーの登録を消す。
    pub async fn cancel(self) -> Result<(), PairingError> {
        self.client
            .delete_pairing(&self.auth, &self.inviter.qr().pairing_id)
            .await?;
        Ok(())
    }
}

/// 新端末側。QR コードを読んで参加を申請し、確認コードを表示してユーザー鍵を受け取る。
pub struct JoinSession {
    client: ServerClient,
    auth: GroupAuth,
    joiner: Joiner,
}

impl JoinSession {
    pub async fn scan(
        client: ServerClient,
        device: DeviceKey,
        qr_text: &str,
    ) -> Result<Self, PairingError> {
        let (joiner, request) = Joiner::scan(device, qr_text)?;
        let auth = GroupAuth::without_token(&joiner.qr().group_id);
        client
            .put_slot(&auth, &joiner.qr().pairing_id, "join", &request)
            .await?;
        Ok(Self {
            client,
            auth,
            joiner,
        })
    }

    /// 承認用ノンスを待ち、署名を確かめて確認コードを返す。
    pub async fn wait_for_code(&mut self) -> Result<SasCode, PairingError> {
        let pairing = self.joiner.qr().pairing_id;
        let challenge: Challenge = loop {
            if let Some(challenge) = self
                .client
                .get_slot(&self.auth, &pairing, "challenge")
                .await?
            {
                break challenge;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        };
        Ok(self.joiner.receive_challenge(&challenge)?)
    }

    /// 既存端末での承認を待ち、グループの秘密を受け取る。
    pub async fn wait_for_welcome(&mut self) -> Result<GroupSecrets, PairingError> {
        let pairing = self.joiner.qr().pairing_id;
        let welcome: Welcome = loop {
            if let Some(welcome) = self
                .client
                .get_slot(&self.auth, &pairing, "welcome")
                .await?
            {
                break welcome;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        };
        Ok(self.joiner.receive_welcome(&welcome)?)
    }
}
