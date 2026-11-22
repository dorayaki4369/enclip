//! 開発用の代替実装（Mac など）。秘密は保護せずに保存するので、開発以外では使わない。

pub struct Clipboard {
    inner: Option<arboard::Clipboard>,
    last_text: Option<String>,
}

impl Clipboard {
    pub fn new() -> Self {
        let mut inner = arboard::Clipboard::new().ok();
        let last_text = inner.as_mut().and_then(|c| c.get_text().ok());
        Self { inner, last_text }
    }

    pub fn poll_change(&mut self) -> Option<String> {
        let text = self.inner.as_mut()?.get_text().ok()?;
        if self.last_text.as_deref() == Some(text.as_str()) {
            return None;
        }
        self.last_text = Some(text.clone());
        Some(text)
    }

    pub fn write_text(&mut self, text: &str) -> Result<(), String> {
        let clipboard = self.inner.as_mut().ok_or("clipboard unavailable")?;
        clipboard
            .set_text(text.to_owned())
            .map_err(|e| e.to_string())?;
        self.last_text = Some(text.to_owned());
        Ok(())
    }
}

pub fn protect(plaintext: &[u8]) -> Result<Vec<u8>, String> {
    Ok(plaintext.to_vec())
}

pub fn unprotect(ciphertext: &[u8]) -> Result<Vec<u8>, String> {
    Ok(ciphertext.to_vec())
}
