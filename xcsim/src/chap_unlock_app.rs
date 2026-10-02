
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    AeadCore, Aes256Gcm, Key, Nonce,
};
use anyhow::{bail, Result};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use tracing::warn;

const CHAP_PREFIX: &str = "@chap/";
const FILENAME: &str = "bm120.fve";

const CHAP_KEY: &[u8; 32] = b"BMChap\x4d\x91\x2c\x7e\xa5\x18\xc3\xd0\x44\x6f\xb1\
                              \x88\x29\x57\xe2\x16\x73\xae\x05\xc9\x6b\x3a\xd1\xf2\x90\x4b";

static UNLOCKED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static DATA_ROOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn store() -> &'static Mutex<HashSet<String>> {
    UNLOCKED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn root_cell() -> &'static Mutex<Option<String>> {
    DATA_ROOT.get_or_init(|| Mutex::new(None))
}

fn cipher() -> Aes256Gcm {
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(CHAP_KEY))
}

fn unlock_path(root: &str) -> String {
    format!("{root}/{FILENAME}")
}

pub fn is_unlocked(local_path: &str) -> bool {
    if !local_path.starts_with(CHAP_PREFIX) {
        return true;
    }
    store().lock().unwrap().contains(local_path)
}

pub fn unlock(local_path: &str) {
    if !local_path.starts_with(CHAP_PREFIX) {
        return;
    }
    let inserted = store().lock().unwrap().insert(local_path.to_owned());
    if inserted {
        if let Err(err) = save() {
            warn!("failed to persist chap unlock: {err:#}");
        }
    }
}

#[allow(dead_code)]
pub fn lock(local_path: &str) {
    let removed = store().lock().unwrap().remove(local_path);
    if removed {
        if let Err(err) = save() {
            warn!("failed to persist chap lock: {err:#}");
        }
    }
}

pub fn reset_all() {
    store().lock().unwrap().clear();
    if let Err(err) = save() {
        warn!("failed to persist chap reset: {err:#}");
    }
}

pub fn load(root: &str) -> Result<()> {
    *root_cell().lock().unwrap() = Some(root.to_owned());

    let path = unlock_path(root);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            store().lock().unwrap().clear();
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    if bytes.len() < 12 {
        bail!("{FILENAME} is corrupted (too short)");
    }
    let nonce = Nonce::from_slice(&bytes[..12]);
    let plaintext = cipher()
        .decrypt(nonce, &bytes[12..])
        .map_err(|_| anyhow::anyhow!("{FILENAME} failed authentication — file may be tampered"))?;

    let set = decode(&plaintext)?;
    *store().lock().unwrap() = set;
    Ok(())
}

pub fn save() -> Result<()> {
    let root_guard = root_cell().lock().unwrap();
    let Some(root) = root_guard.clone() else {
        return Ok(());
    };
    drop(root_guard);

    let plaintext = encode(&store().lock().unwrap());
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher()
        .encrypt(&nonce, plaintext.as_slice())
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;

    let mut file = Vec::with_capacity(12 + ciphertext.len());
    file.extend_from_slice(&nonce);
    file.extend_from_slice(&ciphertext);
    std::fs::write(unlock_path(&root), &file)?;
    Ok(())
}

fn encode(set: &HashSet<String>) -> Vec<u8> {
    let mut entries: Vec<&String> = set.iter().collect();
    entries.sort();
    let mut out = Vec::with_capacity(4 + entries.iter().map(|s| 2 + s.len()).sum::<usize>());
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for s in entries {
        let bytes = s.as_bytes();
        if bytes.len() > u16::MAX as usize {
            continue;
        }
        out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    out
}

fn decode(b: &[u8]) -> Result<HashSet<String>> {
    if b.len() < 4 {
        bail!("{FILENAME} payload too short");
    }
    let count = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
    let mut pos = 4usize;
    let mut set = HashSet::with_capacity(count);
    for _ in 0..count {
        if pos + 2 > b.len() {
            bail!("{FILENAME} truncated at length");
        }
        let len = u16::from_le_bytes([b[pos], b[pos + 1]]) as usize;
        pos += 2;
        if pos + len > b.len() {
            bail!("{FILENAME} truncated at body");
        }
        let s = std::str::from_utf8(&b[pos..pos + len])?.to_owned();
        pos += len;
        set.insert(s);
    }
    Ok(set)
}
