//! Windows のクリップボードと DPAPI。

use std::num::NonZeroU32;

use clipboard_win::{Clipboard as OpenClipboard, raw};
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows::core::PCWSTR;

const OPEN_ATTEMPTS: usize = 10;

/// クリップボードの読み書きと変更の検出。
pub struct Clipboard {
    /// 最後に確認したクリップボードの連番。
    last_seq: Option<NonZeroU32>,
    history_format: Option<NonZeroU32>,
    cloud_format: Option<NonZeroU32>,
}

impl Clipboard {
    pub fn new() -> Self {
        Self {
            last_seq: raw::seq_num(),
            // 受け取った内容を、クリップボード履歴とクラウドクリップボードに残さないための形式。
            history_format: raw::register_format("CanIncludeInClipboardHistory"),
            cloud_format: raw::register_format("CanUploadToCloudClipboard"),
        }
    }

    /// 前回から利用者がコピーしたテキストがあれば返す。自分が書いた変更は返さない。
    pub fn poll_change(&mut self) -> Option<String> {
        let seq = raw::seq_num();
        if seq == self.last_seq {
            return None;
        }
        self.last_seq = seq;
        let _clipboard = OpenClipboard::new_attempts(OPEN_ATTEMPTS).ok()?;
        let mut bytes = Vec::new();
        raw::get_string(&mut bytes).ok()?;
        String::from_utf8(bytes).ok()
    }

    /// 受け取ったテキストをクリップボードへ書く。
    pub fn write_text(&mut self, text: &str) -> Result<(), String> {
        {
            let _clipboard =
                OpenClipboard::new_attempts(OPEN_ATTEMPTS).map_err(|e| e.to_string())?;
            raw::set_string(text).map_err(|e| e.to_string())?;
            let zero = 0u32.to_ne_bytes();
            for format in [self.history_format, self.cloud_format]
                .into_iter()
                .flatten()
            {
                raw::set_without_clear(format.get(), &zero).map_err(|e| e.to_string())?;
            }
        }
        // 自分の書き込みを、次の確認で利用者のコピーとして扱わない。
        self.last_seq = raw::seq_num();
        Ok(())
    }
}

/// 現在の Windows ユーザーだけが復号できる形に暗号化する（DPAPI）。
pub fn protect(plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let input = blob(plaintext)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| e.to_string())?;
        Ok(take_blob(output))
    }
}

pub fn unprotect(ciphertext: &[u8]) -> Result<Vec<u8>, String> {
    let input = blob(ciphertext)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| e.to_string())?;
        Ok(take_blob(output))
    }
}

fn blob(data: &[u8]) -> Result<CRYPT_INTEGER_BLOB, String> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).map_err(|_| "data too large".to_owned())?,
        pbData: data.as_ptr().cast_mut(),
    })
}

/// DPAPI が確保した領域を複製して解放する。
unsafe fn take_blob(blob: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    let bytes = unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(Some(HLOCAL(blob.pbData.cast())));
    }
    bytes
}
