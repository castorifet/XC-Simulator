use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

pub static LOCALE_ALLOWED: AtomicBool = AtomicBool::new(true);
static DETECTED_LOCALE: OnceLock<String> = OnceLock::new();

const ALLOWED: &[&str] = &["en-US", "zh-HK", "zh-TW", "ja-JP", "zh-HANS", "zh-HANT"];

pub fn detected_locale() -> &'static str {
    DETECTED_LOCALE.get().map(|s| s.as_str()).unwrap_or("unknown")
}

pub fn is_allowed() -> bool {
    LOCALE_ALLOWED.load(Ordering::Relaxed)
}

fn normalize(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    let mut parts = raw.split(|c: char| c == '-' || c == '_' || c == '.');
    let lang = parts.next().unwrap_or("").to_ascii_lowercase();
    let region = parts.next().unwrap_or("").to_ascii_uppercase();
    if lang.is_empty() {
        return String::new();
    }
    if region.is_empty() {
        lang
    } else {
        format!("{}-{}", lang, region)
    }
}

fn check_against_allowed(normalized: &str) -> bool {
    ALLOWED.iter().any(|tag| tag.eq_ignore_ascii_case(normalized))
}

pub fn init() {
    let raw = sys_locale::get_locale().unwrap_or_default();
    let normalized = normalize(&raw);
    let allowed = check_against_allowed(&normalized);
    let _ = DETECTED_LOCALE.set(if normalized.is_empty() { "unknown".to_string() } else { normalized });
    LOCALE_ALLOWED.store(allowed, Ordering::Relaxed);
}
