# enclip

Windows と iPhone の間で、クリップボードのテキストをエンドツーエンド暗号化（E2EE）で共有するアプリです。
アカウントは作らず、Windows に表示した QR コードを iPhone で読んで端末を追加します。
いまは日常で使えるかを確かめるための PoC です。

構成と設計判断は [docs/architecture.md](docs/architecture.md)、鍵の仕組みと形式検証は [docs/crypto/key-protocol.md](docs/crypto/key-protocol.md) にあります。

## 構成

| 場所 | 中身 |
|---|---|
| `crates/enclip-core` | 暗号、ペアリングの手順、アイテムの暗号化と受け入れ判定（Rust）。Windows と iPhone で共有する |
| `crates/enclip-client` | 中継サーバーとの通信（Rust）。Windows アプリと開発用 CLI `enclip-dev` が使う |
| `crates/enclip-ffi` | iPhone から `enclip-core` を呼ぶための橋渡し（UniFFI） |
| `apps/server` | 中継サーバー（Cloudflare Workers と Durable Objects）。暗号文を保存して配信するだけで、内容は読めない |
| `apps/windows` | Windows の常駐アプリ（Tauri）。コピーを自動で送り、届いたテキストをクリップボードへ入れる |
| `apps/ios` | iPhone のアプリとキーボード拡張 |

## 動かし方

### 中継サーバー

```sh
cd apps/server
pnpm install
pnpm test          # テスト
pnpm dev           # 手元で起動（http://localhost:8787）
pnpm run deploy    # Cloudflare にデプロイ
```

### Windows アプリ

Windows に Rust と Visual Studio Build Tools（C++ によるデスクトップ開発）を入れてから、リポジトリの直下で実行します。

```sh
cargo build -p enclip-windows --release
```

できた `target/release/enclip-windows.exe` を起動する前に、中継サーバーの URL を `%APPDATA%\app.enclip.desktop\config.json` に書きます。

```json
{ "server_url": "https://<デプロイ先>" }
```

初回の起動でグループが作られます。
iPhone を追加するときは、画面の「端末を追加」を押して QR コードを表示します。

### iPhone アプリ

Mac で次を実行すると、Rust の中核をビルドし、Xcode プロジェクト `apps/ios/EnClip.xcodeproj` を生成します（XcodeGen は nix で一時的に使います）。

```sh
./scripts/setup-ios.sh
```

中継サーバーの URL は `apps/ios/project.yml` の `ENCLIP_SERVER_URL` で指定し、変えたら生成し直します。
Xcode で実機にインストールした後、iPhone の「設定 > 一般 > キーボード」で enclip を追加し、「フルアクセスを許可」をオンにします。
パスワード欄ではカスタムキーボードを使えないので、アプリの履歴をタップしてコピーし、標準のキーボードで貼ります。

### 開発用 CLI

端末 1 台分を模す CLI で、Mac だけでペアリングと送受信を確かめられます。

```sh
cargo run -p enclip-client --bin enclip-dev -- a.json http://localhost:8787 init
cargo run -p enclip-client --bin enclip-dev -- a.json http://localhost:8787 invite
cargo run -p enclip-client --bin enclip-dev -- b.json http://localhost:8787 join '<invite が出した QR の文字列>'
```

状態ファイルは鍵を平文で含むので、開発以外では使いません。

## テスト

```sh
cargo test --workspace                  # Rust（鍵プロトコルの性質を含む）
(cd apps/server && pnpm test)           # 中継サーバー
```
