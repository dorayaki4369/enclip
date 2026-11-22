//! OS ごとに異なる処理（クリップボードと秘密の保存）。
//!
//! 製品の対象は Windows で、Mac などでは開発時に画面と同期を確かめるための代替実装を使う。

#[cfg(not(windows))]
mod other;
#[cfg(windows)]
mod windows;

#[cfg(not(windows))]
pub use other::{Clipboard, protect, unprotect};
#[cfg(windows)]
pub use windows::{Clipboard, protect, unprotect};
