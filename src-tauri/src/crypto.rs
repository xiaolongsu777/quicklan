use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, sync::OnceLock};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::storage;

const KEY_DOMAIN: &[u8] = b"quicklan-chat-v1";
const NONCE_LEN: usize = 12;

/// 聊天消息加密信封：发送方公钥 + 随机数 + 密文（均为 base64）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedEnvelope {
    pub v: u8,
    pub encrypted: bool,
    pub sender_public_key: String,
    pub nonce: String,
    pub ciphertext: String,
}

pub struct DeviceKeys {
    secret: StaticSecret,
    public: PublicKey,
}

static KEYS: OnceLock<DeviceKeys> = OnceLock::new();

pub fn device_keys() -> &'static DeviceKeys {
    KEYS.get_or_init(load_or_create)
}

/// 返回本机 X25519 公钥的 base64 表示，随设备发现广播。
pub fn public_key_b64() -> String {
    B64.encode(device_keys().public.as_bytes())
}

fn load_or_create() -> DeviceKeys {
    let path = key_path();
    if let Ok(encoded) = fs::read_to_string(&path) {
        if let Ok(bytes) = B64.decode(encoded.trim()) {
            if bytes.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                let secret = StaticSecret::from(arr);
                let public = PublicKey::from(&secret);
                return DeviceKeys { secret, public };
            }
        }
    }
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill(&mut bytes);
    let secret = StaticSecret::from(bytes);
    let public = PublicKey::from(&secret);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, B64.encode(bytes));
    DeviceKeys { secret, public }
}

fn key_path() -> PathBuf {
    storage::config_dir().join("device_x25519.key")
}

/// 由 X25519 共享密钥派生 AES-256 会话密钥。
fn derive_key(shared: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(shared);
    hasher.update(KEY_DOMAIN);
    let digest = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    key
}

/// 用对方公钥协商密钥并加密明文，产出加密信封。
pub fn encrypt_for_peer(
    peer_public_key_b64: &str,
    plaintext: &str,
) -> Result<EncryptedEnvelope, String> {
    let keys = device_keys();
    encrypt_with_keys(&keys.secret, &keys.public, peer_public_key_b64, plaintext)
}

/// 使用指定发送方密钥对加密（便于测试与复用）。
fn encrypt_with_keys(
    sender_secret: &StaticSecret,
    sender_public: &PublicKey,
    peer_public_key_b64: &str,
    plaintext: &str,
) -> Result<EncryptedEnvelope, String> {
    let peer_public = decode_public_key(peer_public_key_b64)?;
    let shared = sender_secret.diffie_hellman(&peer_public);
    let key_bytes = derive_key(shared.as_bytes());

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|err| format!("加密聊天消息失败: {err}"))?;

    Ok(EncryptedEnvelope {
        v: 1,
        encrypted: true,
        sender_public_key: B64.encode(sender_public.as_bytes()),
        nonce: B64.encode(nonce_bytes),
        ciphertext: B64.encode(ciphertext),
    })
}

/// 用本机私钥与信封中的发送方公钥协商密钥并解密密文。
pub fn decrypt_envelope(envelope: &EncryptedEnvelope) -> Result<String, String> {
    let keys = device_keys();
    decrypt_with_key(&keys.secret, envelope)
}

/// 使用指定接收方私钥解密（便于测试与复用）。
fn decrypt_with_key(
    receiver_secret: &StaticSecret,
    envelope: &EncryptedEnvelope,
) -> Result<String, String> {
    let sender_public = decode_public_key(&envelope.sender_public_key)?;
    let shared = receiver_secret.diffie_hellman(&sender_public);
    let key_bytes = derive_key(shared.as_bytes());

    let nonce_bytes = B64
        .decode(envelope.nonce.trim())
        .map_err(|err| format!("随机数无效: {err}"))?;
    if nonce_bytes.len() != NONCE_LEN {
        return Err("随机数长度无效".to_string());
    }
    let ciphertext = B64
        .decode(envelope.ciphertext.trim())
        .map_err(|err| format!("密文无效: {err}"))?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_slice())
        .map_err(|_| "解密聊天消息失败".to_string())?;
    String::from_utf8(plaintext).map_err(|err| format!("解密内容非法: {err}"))
}

fn decode_public_key(encoded: &str) -> Result<PublicKey, String> {
    let bytes = B64
        .decode(encoded.trim())
        .map_err(|err| format!("公钥格式无效: {err}"))?;
    if bytes.len() != 32 {
        return Err("公钥长度无效".to_string());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(PublicKey::from(arr))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keypair() -> (StaticSecret, PublicKey) {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill(&mut bytes);
        let secret = StaticSecret::from(bytes);
        let public = PublicKey::from(&secret);
        (secret, public)
    }

    #[test]
    fn roundtrip_between_two_devices() {
        let (alice_secret, alice_public) = keypair();
        let (bob_secret, bob_public) = keypair();
        let bob_pub_b64 = B64.encode(bob_public.as_bytes());
        let message = "你好 QuickLAN，这是一条端到端加密的中文消息！🔒";

        let envelope =
            encrypt_with_keys(&alice_secret, &alice_public, &bob_pub_b64, message).unwrap();
        assert!(envelope.encrypted);
        assert_eq!(envelope.v, 1);
        assert_ne!(envelope.ciphertext, message);

        let decrypted = decrypt_with_key(&bob_secret, &envelope).unwrap();
        assert_eq!(decrypted, message);
    }

    #[test]
    fn third_party_cannot_decrypt() {
        let (alice_secret, alice_public) = keypair();
        let (_bob_secret, bob_public) = keypair();
        let (mallory_secret, _) = keypair();
        let bob_pub_b64 = B64.encode(bob_public.as_bytes());

        let envelope =
            encrypt_with_keys(&alice_secret, &alice_public, &bob_pub_b64, "secret payload")
                .unwrap();
        assert!(decrypt_with_key(&mallory_secret, &envelope).is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let (alice_secret, alice_public) = keypair();
        let (bob_secret, bob_public) = keypair();
        let bob_pub_b64 = B64.encode(bob_public.as_bytes());

        let mut envelope =
            encrypt_with_keys(&alice_secret, &alice_public, &bob_pub_b64, "hello").unwrap();
        let mut raw = B64.decode(&envelope.ciphertext).unwrap();
        raw[0] ^= 0xFF;
        envelope.ciphertext = B64.encode(raw);

        assert!(decrypt_with_key(&bob_secret, &envelope).is_err());
    }

    #[test]
    fn envelope_serializes_as_json() {
        let (alice_secret, alice_public) = keypair();
        let (_bob_secret, bob_public) = keypair();
        let bob_pub_b64 = B64.encode(bob_public.as_bytes());

        let envelope =
            encrypt_with_keys(&alice_secret, &alice_public, &bob_pub_b64, "payload").unwrap();
        let json = serde_json::to_string(&envelope).unwrap();
        assert!(json.contains("\"encrypted\":true"));
        let parsed: EncryptedEnvelope = serde_json::from_str(&json).unwrap();
        assert!(parsed.encrypted);
    }
}
