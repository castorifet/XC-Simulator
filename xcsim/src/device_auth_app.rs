use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicI8, Ordering};
use std::sync::{Mutex, OnceLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};

pub const AUTH_HOST: &str = "auth2.luvxcby.love";
pub const AUTH_PORT: u16 = 26111;
const AUTH_TIMEOUT_SECS: u64 = 10;

pub const AUTH_PENDING: i8 = -1;
pub const AUTH_DENIED: i8 = 0;
pub const AUTH_OK: i8 = 1;

pub static AUTH_STATE: AtomicI8 = AtomicI8::new(AUTH_PENDING);
static DEVICE_ID: OnceLock<String> = OnceLock::new();

static BAN_REASON: Mutex<Option<String>> = Mutex::new(None);
static BAN_DURATION: Mutex<Option<String>> = Mutex::new(None);

static BAN_UNTIL: AtomicI64 = AtomicI64::new(-1);

pub fn device_id() -> &'static str {
    DEVICE_ID.get().map(|s| s.as_str()).unwrap_or("")
}

pub fn ban_reason() -> Option<String> {
    BAN_REASON.lock().unwrap().clone()
}

pub fn ban_duration() -> Option<String> {
    BAN_DURATION.lock().unwrap().clone()
}

pub fn ban_until() -> i64 {
    BAN_UNTIL.load(Ordering::Relaxed)
}

fn set_ban_info(until: i64, duration: Option<String>, reason: Option<String>) {
    BAN_UNTIL.store(until, Ordering::Relaxed);
    *BAN_DURATION.lock().unwrap() = duration.filter(|s| !s.is_empty());
    *BAN_REASON.lock().unwrap() = reason.filter(|s| !s.is_empty());
}

fn clear_ban_info() {
    BAN_UNTIL.store(-1, Ordering::Relaxed);
    *BAN_DURATION.lock().unwrap() = None;
    *BAN_REASON.lock().unwrap() = None;
}

pub fn auth_state() -> i8 {
    AUTH_STATE.load(Ordering::Relaxed)
}

pub fn is_authorized() -> bool {
    auth_state() == AUTH_OK
}

pub fn is_pending() -> bool {
    auth_state() == AUTH_PENDING
}

pub fn is_denied() -> bool {
    auth_state() == AUTH_DENIED
}

fn id_path() -> Result<PathBuf> {
    Ok(PathBuf::from(crate::dir::root()?).join("device_id"))
}

fn valid_id(s: &str) -> bool {
    s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit())
}

fn gen_id() -> String {
    let n: u32 = ::rand::random::<u32>() % 100_000_000;
    format!("{:08}", n)
}

pub fn load_id() -> Result<String> {
    let path = id_path()?;
    if path.exists() {
        let id = std::fs::read_to_string(&path)?.trim().to_owned();
        if valid_id(&id) {
            return Ok(id);
        }
    }
    let new_id = gen_id();
    std::fs::write(&path, &new_id)?;
    Ok(new_id)
}

pub fn init_id() -> String {
    let id = load_id().unwrap_or_else(|_| gen_id());
    let _ = DEVICE_ID.set(id.clone());
    id
}

pub async fn auth(id: String) -> Result<bool> {
    let fut = async {
        let mut stream = TcpStream::connect((AUTH_HOST, AUTH_PORT)).await?;
        let msg = format!("AUTH {}\n", id);
        stream.write_all(msg.as_bytes()).await?;
        let mut buf = vec![0u8; 512];
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok::<bool, anyhow::Error>(false);
        }
        let resp = String::from_utf8_lossy(&buf[..n]);
        let line = resp.lines().next().unwrap_or("").trim();
        let mut parts = line.split('\t');
        let tag = parts.next().unwrap_or("").trim();
        if tag.eq_ignore_ascii_case("OK") {
            clear_ban_info();
            return Ok::<bool, anyhow::Error>(true);
        }

        let until: i64 = parts.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
        let duration = parts.next().map(|s| s.trim().to_string());
        let reason = parts.next().map(|s| s.trim().to_string());
        set_ban_info(until, duration, reason);
        Ok::<bool, anyhow::Error>(false)
    };
    match timeout(Duration::from_secs(AUTH_TIMEOUT_SECS), fut).await {
        Ok(res) => res,
        Err(_) => Err(anyhow!("auth timeout")),
    }
}

pub fn set_state(state: i8) {
    AUTH_STATE.store(state, Ordering::Relaxed);
}
