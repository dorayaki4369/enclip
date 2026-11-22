//! 中継サーバーの HTTP API と WebSocket。

use enclip_core::{GroupSecrets, ItemEnvelope, PairingId, encode_b64};
use futures_util::StreamExt;
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use thiserror::Error;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};
use url::Url;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("network error: {0}")]
    Network(String),
    #[error("server returned {0}")]
    Status(u16),
    #[error("unexpected response: {0}")]
    Response(String),
}

impl From<reqwest::Error> for ApiError {
    fn from(error: reqwest::Error) -> Self {
        Self::Network(error.to_string())
    }
}

/// サーバーへ渡すグループの宛先とアクセス用トークン。
#[derive(Clone)]
pub struct GroupAuth {
    group_id: String,
    token: String,
}

impl GroupAuth {
    pub fn from_secrets(secrets: &GroupSecrets) -> Self {
        Self {
            group_id: encode_b64(&secrets.group_id.0),
            token: encode_b64(&secrets.access_token.0),
        }
    }

    /// トークンを持たない新端末が、QR コードのグループ ID だけで使う形。
    pub(crate) fn without_token(group_id: &enclip_core::GroupId) -> Self {
        Self {
            group_id: encode_b64(&group_id.0),
            token: String::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ItemsPage {
    items: Vec<SeqItem>,
}

/// サーバーが付けた連番とアイテム。
#[derive(Debug, Clone, Deserialize)]
pub struct SeqItem {
    pub seq: u64,
    pub envelope: ItemEnvelope,
}

#[derive(Debug, Deserialize)]
struct PostedItem {
    seq: u64,
}

#[derive(Clone)]
pub struct ServerClient {
    base: Url,
    http: reqwest::Client,
}

impl ServerClient {
    pub fn new(base: Url) -> Self {
        Self {
            base,
            http: reqwest::Client::new(),
        }
    }

    fn url(&self, auth: &GroupAuth, path: &str) -> Url {
        let mut url = self.base.clone();
        url.set_path(&format!("/groups/{}{}", auth.group_id, path));
        url
    }

    async fn send(
        &self,
        method: Method,
        url: Url,
        auth: &GroupAuth,
        body: Option<&impl serde::Serialize>,
    ) -> Result<reqwest::Response, ApiError> {
        let mut request = self.http.request(method, url);
        if !auth.token.is_empty() {
            request = request.bearer_auth(&auth.token);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        Ok(request.send().await?)
    }

    async fn expect_status(
        response: reqwest::Response,
        ok: &[StatusCode],
    ) -> Result<reqwest::Response, ApiError> {
        if ok.contains(&response.status()) {
            Ok(response)
        } else {
            Err(ApiError::Status(response.status().as_u16()))
        }
    }

    /// グループをサーバーに登録する。同じトークンでの再登録は成功として扱う。
    pub async fn register_group(&self, auth: &GroupAuth) -> Result<(), ApiError> {
        let response = self
            .send(Method::PUT, self.url(auth, ""), auth, None::<&()>)
            .await?;
        Self::expect_status(response, &[StatusCode::CREATED, StatusCode::NO_CONTENT]).await?;
        Ok(())
    }

    pub async fn post_item(
        &self,
        auth: &GroupAuth,
        envelope: &ItemEnvelope,
    ) -> Result<u64, ApiError> {
        let response = self
            .send(Method::POST, self.url(auth, "/items"), auth, Some(envelope))
            .await?;
        let response = Self::expect_status(response, &[StatusCode::CREATED]).await?;
        Ok(response.json::<PostedItem>().await?.seq)
    }

    /// 連番 `after` より後のアイテムを返す。1 回で返る件数には上限がある。
    pub async fn items_after(
        &self,
        auth: &GroupAuth,
        after: u64,
    ) -> Result<Vec<SeqItem>, ApiError> {
        let mut url = self.url(auth, "/items");
        url.query_pairs_mut()
            .append_pair("after", &after.to_string());
        let response = self.send(Method::GET, url, auth, None::<&()>).await?;
        let response = Self::expect_status(response, &[StatusCode::OK]).await?;
        Ok(response.json::<ItemsPage>().await?.items)
    }

    /// 新しいアイテムの配信を受ける WebSocket を開く。
    pub async fn subscribe(&self, auth: &GroupAuth) -> Result<ItemSocket, ApiError> {
        let mut url = self.url(auth, "/ws");
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme)
            .map_err(|_| ApiError::Response("websocket scheme".into()))?;
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|error| ApiError::Network(error.to_string()))?;
        let header = HeaderValue::from_str(&format!("Bearer {}", auth.token))
            .map_err(|_| ApiError::Response("token header".into()))?;
        request.headers_mut().insert("Authorization", header);
        let (socket, _) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        Ok(ItemSocket { socket })
    }

    pub async fn create_pairing(
        &self,
        auth: &GroupAuth,
        pairing: &PairingId,
    ) -> Result<(), ApiError> {
        let response = self
            .send(
                Method::PUT,
                self.pairing_url(auth, pairing, None),
                auth,
                None::<&()>,
            )
            .await?;
        Self::expect_status(response, &[StatusCode::CREATED]).await?;
        Ok(())
    }

    pub async fn delete_pairing(
        &self,
        auth: &GroupAuth,
        pairing: &PairingId,
    ) -> Result<(), ApiError> {
        let response = self
            .send(
                Method::DELETE,
                self.pairing_url(auth, pairing, None),
                auth,
                None::<&()>,
            )
            .await?;
        Self::expect_status(response, &[StatusCode::NO_CONTENT]).await?;
        Ok(())
    }

    pub(crate) async fn put_slot(
        &self,
        auth: &GroupAuth,
        pairing: &PairingId,
        slot: &str,
        message: &impl serde::Serialize,
    ) -> Result<(), ApiError> {
        let response = self
            .send(
                Method::PUT,
                self.pairing_url(auth, pairing, Some(slot)),
                auth,
                Some(message),
            )
            .await?;
        Self::expect_status(response, &[StatusCode::CREATED]).await?;
        Ok(())
    }

    /// 欄がまだ書かれていなければ `None`。
    pub(crate) async fn get_slot<T: DeserializeOwned>(
        &self,
        auth: &GroupAuth,
        pairing: &PairingId,
        slot: &str,
    ) -> Result<Option<T>, ApiError> {
        let response = self
            .send(
                Method::GET,
                self.pairing_url(auth, pairing, Some(slot)),
                auth,
                None::<&()>,
            )
            .await?;
        match response.status() {
            StatusCode::OK => Ok(Some(response.json().await?)),
            StatusCode::NO_CONTENT => Ok(None),
            status => Err(ApiError::Status(status.as_u16())),
        }
    }

    fn pairing_url(&self, auth: &GroupAuth, pairing: &PairingId, slot: Option<&str>) -> Url {
        let path = match slot {
            Some(slot) => format!("/pairings/{}/{slot}", encode_b64(&pairing.0)),
            None => format!("/pairings/{}", encode_b64(&pairing.0)),
        };
        self.url(auth, &path)
    }
}

/// サーバーからのアイテム配信。
pub struct ItemSocket {
    socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
}

impl ItemSocket {
    /// 次のアイテムを待つ。接続が切れたら `None`。
    pub async fn next(&mut self) -> Option<Result<SeqItem, ApiError>> {
        while let Some(message) = self.socket.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    return Some(
                        serde_json::from_str::<SeqItem>(&text)
                            .map_err(|error| ApiError::Response(error.to_string())),
                    );
                }
                Ok(Message::Close(_)) => return None,
                Ok(_) => continue,
                Err(error) => return Some(Err(ApiError::Network(error.to_string()))),
            }
        }
        None
    }
}
