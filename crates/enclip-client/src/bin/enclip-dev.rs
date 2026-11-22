//! 開発用の CLI。Mac 上で端末 1 台分を模して、ペアリングと送受信を通しで確かめる。
//!
//! 状態ファイルは平文の JSON で保存するので、開発以外では使わない。
//!
//! ```sh
//! enclip-dev <state.json> <server-url> init            # 端末とグループを作る
//! enclip-dev <state.json> <server-url> invite [--yes]  # QR の文字列を出し、参加を承認する
//! enclip-dev <state.json> <server-url> join <qr>       # QR の文字列で参加する
//! enclip-dev <state.json> <server-url> send <text>     # テキストを送る
//! enclip-dev <state.json> <server-url> fetch           # 新しいアイテムを取得する
//! enclip-dev <state.json> <server-url> watch           # 配信を待ち受けて表示する
//! ```

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use enclip_client::{
    DeviceState, GroupAuth, GroupState, InviteSession, JoinSession, ServerClient, Url, fetch_new,
    send_text,
};
use enclip_core::GroupSecrets;

const ITEM_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

type CliResult<T> = Result<T, String>;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: &[String]) -> CliResult<()> {
    let [state_path, server, command, rest @ ..] = args else {
        return Err("usage: enclip-dev <state.json> <server-url> <command> [args]".into());
    };
    let state_path = PathBuf::from(state_path);
    let client = ServerClient::new(Url::parse(server).map_err(|e| e.to_string())?);
    match (command.as_str(), rest) {
        ("init", []) => init(&state_path, &client).await,
        ("invite", flags) => invite(&state_path, &client, flags.iter().any(|f| f == "--yes")).await,
        ("join", [qr]) => join(&state_path, &client, qr).await,
        ("send", [text]) => send(&state_path, &client, text).await,
        ("fetch", []) => fetch(&state_path, &client).await,
        ("watch", []) => watch(&state_path, &client).await,
        _ => Err(format!("unknown command: {command}")),
    }
}

async fn init(path: &Path, client: &ServerClient) -> CliResult<()> {
    let mut state = DeviceState::generate().map_err(|e| e.to_string())?;
    let secrets = GroupSecrets::create().map_err(|e| e.to_string())?;
    client
        .register_group(&GroupAuth::from_secrets(&secrets))
        .await
        .map_err(|e| e.to_string())?;
    state.group = Some(GroupState::new(&secrets));
    save(path, &state)?;
    println!("group created");
    Ok(())
}

async fn invite(path: &Path, client: &ServerClient, auto_approve: bool) -> CliResult<()> {
    let state = load(path)?;
    let group = group(&state)?.secrets().map_err(|e| e.to_string())?;
    let device = state.device_key().map_err(|e| e.to_string())?;
    let mut session = InviteSession::start(client.clone(), device, group)
        .await
        .map_err(|e| e.to_string())?;
    println!("qr: {}", session.qr_text());
    let sas = session.wait_for_join().await.map_err(|e| e.to_string())?;
    println!("code: {}", sas.0);
    if !auto_approve {
        print!("approve? [y/N] ");
        std::io::stdout().flush().map_err(|e| e.to_string())?;
        let mut answer = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut answer)
            .map_err(|e| e.to_string())?;
        if answer.trim() != "y" {
            session.cancel().await.map_err(|e| e.to_string())?;
            return Err("not approved".into());
        }
    }
    session.approve().await.map_err(|e| e.to_string())?;
    println!("approved");
    Ok(())
}

async fn join(path: &Path, client: &ServerClient, qr: &str) -> CliResult<()> {
    let mut state = DeviceState::generate().map_err(|e| e.to_string())?;
    let device = state.device_key().map_err(|e| e.to_string())?;
    let mut session = JoinSession::scan(client.clone(), device, qr)
        .await
        .map_err(|e| e.to_string())?;
    let sas = session.wait_for_code().await.map_err(|e| e.to_string())?;
    println!("code: {}", sas.0);
    let secrets = session
        .wait_for_welcome()
        .await
        .map_err(|e| e.to_string())?;
    state.group = Some(GroupState::new(&secrets));
    save(path, &state)?;
    println!("joined");
    Ok(())
}

async fn send(path: &Path, client: &ServerClient, text: &str) -> CliResult<()> {
    let state = load(path)?;
    let group = group(&state)?.secrets().map_err(|e| e.to_string())?;
    let me = state.device_key().map_err(|e| e.to_string())?.public();
    let ttl = u64::try_from(ITEM_TTL.as_millis()).expect("ttl fits in u64");
    let seq = send_text(client, &group, &me, text, now_ms(), ttl)
        .await
        .map_err(|e| e.to_string())?;
    println!("sent seq={seq}");
    Ok(())
}

async fn fetch(path: &Path, client: &ServerClient) -> CliResult<()> {
    let mut state = load(path)?;
    let me = state.device_key().map_err(|e| e.to_string())?.public();
    let group = state.group.as_mut().ok_or("no group")?;
    let received = fetch_new(client, group, &me, now_ms())
        .await
        .map_err(|e| e.to_string())?;
    save(path, &state)?;
    for item in received {
        println!(
            "received seq={} self={} text={}",
            item.seq, item.from_self, item.payload.text
        );
    }
    Ok(())
}

async fn watch(path: &Path, client: &ServerClient) -> CliResult<()> {
    let state = load(path)?;
    let auth = GroupAuth::from_secrets(&group(&state)?.secrets().map_err(|e| e.to_string())?);
    let mut socket = client.subscribe(&auth).await.map_err(|e| e.to_string())?;
    println!("watching");
    fetch(path, client).await?;
    while let Some(next) = socket.next().await {
        next.map_err(|e| e.to_string())?;
        fetch(path, client).await?;
    }
    Ok(())
}

fn group(state: &DeviceState) -> CliResult<&GroupState> {
    state.group.as_ref().ok_or_else(|| "no group".to_owned())
}

fn now_ms() -> u64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is after 1970");
    u64::try_from(elapsed.as_millis()).expect("time fits in u64")
}

fn load(path: &Path) -> CliResult<DeviceState> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn save(path: &Path, state: &DeviceState) -> CliResult<()> {
    let text = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}
