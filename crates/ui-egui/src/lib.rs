//! CleanCraft egui frontend.

pub mod app;
pub mod file_icons;
pub mod heroes;
pub mod icons;
pub mod images;
pub mod jobs;
pub mod kit;
pub mod kit_list;
pub mod logo;
pub mod menubar_panel;
pub mod motion;
pub mod pages;
pub mod render3d;
pub mod section_icons;
pub mod sound;
pub mod theme;
pub mod titlebar;

pub use app::{CleanApp, Page};

/// `text` in the interface language (`cleancraft_core::i18n`; English when untranslated).
pub(crate) fn tr(text: impl ToString) -> String {
    let s = text.to_string();
    let out = match cleancraft_core::i18n::tr(&s) {
        std::borrow::Cow::Borrowed(b) if std::ptr::eq(b, s.as_str()) => None,
        t => Some(t.into_owned()),
    };
    out.unwrap_or(s)
}

/// [`egui::RichText`] of `text` in the interface language.
pub(crate) fn rich(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(tr(text.into()))
}
