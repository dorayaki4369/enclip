//! クリップボードを専用のスレッドで扱う。
//!
//! 利用者のコピーを一定間隔で確かめて通知し、受け取ったテキストの書き込み要求を処理する。

use std::sync::mpsc;
use std::time::Duration;

use tokio::sync::mpsc as async_mpsc;

use crate::platform::Clipboard;

const POLL_INTERVAL: Duration = Duration::from_millis(300);
/// クリップボードのテキストとして送る上限（暗号化前）。
pub const MAX_TEXT_BYTES: usize = 128 * 1024;

#[derive(Clone)]
pub struct ClipboardHandle {
    writes: mpsc::Sender<String>,
}

impl ClipboardHandle {
    pub fn write(&self, text: String) {
        // スレッドが止まっている場合は書けないが、アプリの終了時以外には起きない。
        let _ = self.writes.send(text);
    }
}

/// スレッドを起動し、利用者のコピーを受け取るチャネルを返す。
pub fn spawn() -> (ClipboardHandle, async_mpsc::UnboundedReceiver<String>) {
    let (write_tx, write_rx) = mpsc::channel::<String>();
    let (copy_tx, copy_rx) = async_mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        let mut clipboard = Clipboard::new();
        loop {
            match write_rx.recv_timeout(POLL_INTERVAL) {
                Ok(text) => {
                    if let Err(error) = clipboard.write_text(&text) {
                        eprintln!("clipboard write failed: {error}");
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            if let Some(text) = clipboard.poll_change()
                && !text.is_empty()
                && text.len() <= MAX_TEXT_BYTES
                && copy_tx.send(text).is_err()
            {
                return;
            }
        }
    });
    (ClipboardHandle { writes: write_tx }, copy_rx)
}
