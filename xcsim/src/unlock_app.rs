use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    AeadCore, Aes256Gcm, Key, Nonce,
};
use anyhow::{bail, Result};
use std::sync::{atomic::{AtomicBool, Ordering}, OnceLock};

include!(concat!(env!("OUT_DIR"), "/unlock_ids.rs"));

const FEAT_ID_KEY: &[u8; 32] = b"FeatId\xb8\x2f\x7a\xc1\x55\x3e\x90\xd4\x08\x6b\xf2\
                                  \x19\x4c\x87\xe3\x2a\x71\xbc\x0f\x9d\x64\x38\xc5\xe7\xa1\xd2";

static DECRYPTED_IDS: OnceLock<Vec<(u32, &'static str)>> = OnceLock::new();

pub fn unlock_ids() -> &'static [(u32, &'static str)] {
    DECRYPTED_IDS.get_or_init(|| {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(FEAT_ID_KEY));
        ENCRYPTED_FEAT_IDS
            .iter()
            .map(|(nonce_bytes, ciphertext, label)| {
                let nonce = Nonce::from_slice(nonce_bytes);
                let plain = cipher.decrypt(nonce, *ciphertext)
                    .expect("feature ID decryption failed — binary may be corrupted");
                let code = u32::from_le_bytes(plain.try_into().expect("bad plaintext length"));
                (code, *label)
            })
            .collect()
    })
}

const RAW_KEY: &[u8; 32] = b"Xhigros\xf7\x3a\x91\xb2\x4e\xc8\xd0\x55\x7f\xa3\x19\x2b\
                              \xe6\x40\x8c\xf1\x6d\x0a\x74\xb9\x3e\x27\xcc\x85\xfe";

pub static FEAT_BG_CHANGE: AtomicBool = AtomicBool::new(false);
pub static FEAT_BGM_CHANGE: AtomicBool = AtomicBool::new(false);

#[derive(Default, Clone)]
pub struct UnlockState {
    pub bg_change: bool,
    pub bgm_change: bool,
    pub bg_path: Option<String>,
    pub bgm_path: Option<String>,
}

static STATE: std::sync::Mutex<Option<UnlockState>> = std::sync::Mutex::new(None);

pub fn get_state() -> UnlockState {
    STATE.lock().unwrap().clone().unwrap_or_default()
}

pub fn set_state(s: UnlockState) {
    FEAT_BG_CHANGE.store(s.bg_change, Ordering::Relaxed);
    FEAT_BGM_CHANGE.store(s.bgm_change, Ordering::Relaxed);
    *STATE.lock().unwrap() = Some(s);
}

fn unlock_path(root: &str) -> String {
    format!("{root}/uc120.fve")
}

fn cipher() -> Aes256Gcm {
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(RAW_KEY))
}

pub fn load(root: &str) -> Result<()> {
    let path = unlock_path(root);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            set_state(UnlockState::default());
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    if bytes.len() < 12 {
        bail!("uc120.fve is corrupted (too short)");
    }
    let nonce = Nonce::from_slice(&bytes[..12]);
    let plaintext = cipher()
        .decrypt(nonce, &bytes[12..])
        .map_err(|_| anyhow::anyhow!("uc120.fve failed authentication — file may be tampered"))?;

    let state = decode_state(&plaintext)?;
    set_state(state);
    Ok(())
}

pub fn save(root: &str) -> Result<()> {
    let state = get_state();
    let plaintext = encode_state(&state);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher()
        .encrypt(&nonce, plaintext.as_slice())
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;

    let mut file = Vec::with_capacity(12 + ciphertext.len());
    file.extend_from_slice(&nonce);
    file.extend_from_slice(&ciphertext);
    std::fs::write(unlock_path(root), &file)?;
    Ok(())
}

fn encode_state(s: &UnlockState) -> Vec<u8> {
    let flags: u8 = (s.bg_change as u8) | ((s.bgm_change as u8) << 1);
    let bg = s.bg_path.as_deref().unwrap_or("").as_bytes();
    let bgm = s.bgm_path.as_deref().unwrap_or("").as_bytes();
    let mut out = Vec::with_capacity(1 + 2 + bg.len() + 2 + bgm.len());
    out.push(flags);
    out.extend_from_slice(&(bg.len() as u16).to_le_bytes());
    out.extend_from_slice(bg);
    out.extend_from_slice(&(bgm.len() as u16).to_le_bytes());
    out.extend_from_slice(bgm);
    out
}

fn decode_state(b: &[u8]) -> Result<UnlockState> {
    if b.is_empty() {
        bail!("uc120.fve payload empty");
    }
    let flags = b[0];
    let bg_change = flags & 1 != 0;
    let bgm_change = flags & 2 != 0;

    let mut pos = 1usize;
    let bg_path = read_str(b, &mut pos)?;
    let bgm_path = read_str(b, &mut pos)?;

    Ok(UnlockState {
        bg_change,
        bgm_change,
        bg_path: if bg_path.is_empty() { None } else { Some(bg_path) },
        bgm_path: if bgm_path.is_empty() { None } else { Some(bgm_path) },
    })
}

fn read_str(b: &[u8], pos: &mut usize) -> Result<String> {
    if *pos + 2 > b.len() {
        bail!("uc120.fve truncated at string length");
    }
    let len = u16::from_le_bytes([b[*pos], b[*pos + 1]]) as usize;
    *pos += 2;
    if *pos + len > b.len() {
        bail!("uc120.fve truncated at string body");
    }
    let s = std::str::from_utf8(&b[*pos..*pos + len])?.to_owned();
    *pos += len;
    Ok(s)
}
