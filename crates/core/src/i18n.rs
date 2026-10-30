//! Interface languages. CleanCraft is written in English; [`tr`] shows a text in the chosen
//! language when its table has it, and the English text otherwise, so a missing translation
//! never hides anything.
//!
//! A table maps the English text to the translated one. A key with `{}` is a template: each
//! `{}` stands for a part that changes ("{} apps"), and the parts are put back, in the same
//! order, where the translation has its `{}`.
//!
//! The language is chosen once per process by the app and the menu bar agent ([`apply`]):
//! the Settings choice, else the first of the Mac's preferred languages that has a table,
//! else English. It is never changed on a fake Mac, so tests always see English.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::env::Env;
use crate::settings::Settings;

mod it;

/// The languages CleanCraft speaks: code and name (in that language).
pub const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("it", "Italiano")];

/// Index into [`LANGUAGES`].
static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The language in use ("en", "it").
pub fn current() -> &'static str {
    LANGUAGES[CURRENT.load(Ordering::Relaxed) as usize].0
}

/// Use `code` from now on (an unknown code means English).
pub fn set(code: &str) {
    let i = LANGUAGES.iter().position(|(c, _)| *c == code).unwrap_or(0);
    CURRENT.store(i as u8, Ordering::Relaxed);
}

/// The language for this setting ("" follows the Mac) and the Mac's preferred languages
/// (codes, most preferred first).
pub fn resolve(setting: &str, preferred: &[String]) -> &'static str {
    let known = |c: &str| LANGUAGES.iter().find(|(k, _)| *k == c).map(|(k, _)| *k);
    if let Some(c) = known(setting.trim()) {
        return c;
    }
    preferred.iter().find_map(|p| known(p)).unwrap_or("en")
}

/// The Mac's preferred languages, most preferred first (empty when they cannot be read).
pub fn preferred(env: &Env) -> Vec<String> {
    match env
        .sys
        .run("/usr/bin/defaults", &["read", "-g", "AppleLanguages"])
    {
        Ok(out) if out.ok() => crate::features::system_junk::parse_apple_languages(&out.stdout),
        _ => Vec::new(),
    }
}

/// Choose the language for this process from the settings (never on a fake Mac).
pub fn apply(env: &Env, settings: &Settings) {
    if env.is_fake() {
        return;
    }
    let pref = if settings.language.trim().is_empty() {
        preferred(env)
    } else {
        Vec::new()
    };
    set(resolve(&settings.language, &pref));
}

struct Table {
    exact: HashMap<&'static str, &'static str>,
    /// Templates split at `{}`, the most specific (longest fixed text) first.
    templates: Vec<(Vec<&'static str>, &'static str)>,
}

fn table(pairs: &'static [(&'static str, &'static str)]) -> Table {
    let mut exact = HashMap::new();
    let mut templates = Vec::new();
    for (en, tr) in pairs {
        if en.contains("{}") {
            templates.push((en.split("{}").collect::<Vec<_>>(), *tr));
        } else {
            exact.insert(*en, *tr);
        }
    }
    templates.sort_by_key(|(parts, _): &(Vec<&str>, &str)| {
        std::cmp::Reverse(parts.iter().map(|p| p.len()).sum::<usize>())
    });
    Table { exact, templates }
}

fn current_table() -> Option<&'static Table> {
    static IT: OnceLock<Table> = OnceLock::new();
    match current() {
        "it" => Some(IT.get_or_init(|| table(it::TEXTS))),
        _ => None,
    }
}

/// `text` in the language in use (the English text when the table lacks it).
pub fn tr(text: &str) -> Cow<'_, str> {
    match current_table() {
        Some(t) => translate(t, text, 3),
        None => Cow::Borrowed(text),
    }
}

/// `text` from table `t`. The parts that fill a template's holes are looked up too (`depth`
/// more levels), so "Mail Downloads (2)" follows "{} ({})" and "Mail Downloads".
fn translate<'a>(t: &Table, text: &'a str, depth: u8) -> Cow<'a, str> {
    if let Some(s) = t.exact.get(text) {
        return Cow::Borrowed(s);
    }
    if depth == 0 {
        return Cow::Borrowed(text);
    }
    for (parts, tr) in &t.templates {
        if let Some(holes) = fill(parts, text) {
            let mut out = String::with_capacity(tr.len() + text.len());
            let mut holes = holes.into_iter();
            for (i, piece) in tr.split("{}").enumerate() {
                if i > 0 {
                    let h = holes.next().unwrap_or_default();
                    out.push_str(&translate(t, h, depth - 1));
                }
                out.push_str(piece);
            }
            return Cow::Owned(out);
        }
    }
    Cow::Borrowed(text)
}

/// The parts of `text` that stand where `parts` (a template split at `{}`) has its holes;
/// None when `text` does not follow the template. Each hole holds at least one character.
fn fill<'a>(parts: &[&str], text: &'a str) -> Option<Vec<&'a str>> {
    let (first, rest) = parts.split_first()?;
    let (last, middle) = rest.split_last()?;
    let mut s = text.strip_prefix(first)?.strip_suffix(last)?;
    let mut holes = Vec::with_capacity(parts.len() - 1);
    for m in middle {
        // The hole before `m` is the shortest non-empty one.
        let skip = s.chars().next()?.len_utf8();
        let at = s[skip..].find(m)? + skip;
        holes.push(&s[..at]);
        s = &s[at + m.len()..];
    }
    if s.is_empty() {
        return None;
    }
    holes.push(s);
    Some(holes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(text: &str) -> String {
        static T: OnceLock<Table> = OnceLock::new();
        translate(T.get_or_init(|| table(it::TEXTS)), text, 3).into_owned()
    }

    #[test]
    fn english_until_set_and_every_table_is_well_formed() {
        assert_eq!(tr("Settings"), "Settings");
        let mut seen = std::collections::HashSet::new();
        for (en, tr) in it::TEXTS {
            assert!(seen.insert(*en), "twice in it.rs: {en}");
            assert!(!tr.trim().is_empty(), "empty translation for {en}");
            assert_eq!(
                en.matches("{}").count(),
                tr.matches("{}").count(),
                "holes differ: {en}"
            );
            assert!(!tr.contains('\u{2014}'), "em dash in: {tr}");
            assert!(!en.contains("{}{}"), "two holes side by side: {en}");
        }
    }

    #[test]
    fn exact_texts_and_templates() {
        assert_eq!(it("Settings"), "Impostazioni");
        assert_eq!(
            it("Something not in the table"),
            "Something not in the table"
        );
        // A template; the hole is translated too when the table has it.
        assert_eq!(
            it("Review: Live Monitor"),
            "Da vedere: Monitor in tempo reale"
        );
        assert_eq!(it("12 apps"), "12 app");
        // The whole text must follow the template.
        assert_eq!(it("apps"), "apps");
    }

    #[test]
    fn holes_are_found_in_order() {
        let parts: Vec<&str> = "{} of {} free".split("{}").collect();
        assert_eq!(
            fill(&parts, "1 GB of 2 GB free"),
            Some(vec!["1 GB", "2 GB"])
        );
        assert_eq!(fill(&parts, " of 2 GB free"), None);
        assert_eq!(fill(&parts, "1 GB of  free"), None);
    }

    #[test]
    fn the_setting_wins_then_the_macs_order() {
        let mac = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(resolve("it", &mac(&["en"])), "it");
        assert_eq!(resolve("", &mac(&["fr", "it", "en"])), "it");
        assert_eq!(resolve("", &mac(&["en", "it"])), "en");
        assert_eq!(resolve("xx", &mac(&["de"])), "en");
    }

    #[test]
    fn a_fake_mac_stays_in_english() {
        let mac = crate::testkit::FakeMac::new();
        let s = Settings {
            language: "it".into(),
            ..Settings::default()
        };
        apply(&mac.env, &s);
        assert_eq!(current(), "en");
    }
}
