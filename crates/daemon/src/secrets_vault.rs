use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine as _;
use ring::aead;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const KEY_FILE_NAME: &str = "agent_secrets.key";
const DATA_FILE_NAME: &str = "agent_secrets.v1.json";
const NONCE_LEN: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct VaultFile {
    #[serde(default)]
    secrets: BTreeMap<String, EncryptedSecret>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedSecret {
    nonce_b64: String,
    ciphertext_b64: String,
}

/// Local encrypted storage for API keys and other agent secrets.
pub struct SecretsVault {
    data_path: PathBuf,
    key_bytes: [u8; 32],
}

impl SecretsVault {
    pub fn open(config_dir: &Path) -> Result<Self, io::Error> {
        fs::create_dir_all(config_dir)?;
        let key_path = config_dir.join(KEY_FILE_NAME);
        let data_path = config_dir.join(DATA_FILE_NAME);
        let key_bytes = read_or_create_key(&key_path)?;
        Ok(Self {
            data_path,
            key_bytes,
        })
    }

    pub fn set_secret(&self, key: &str, value: &str) -> Result<(), io::Error> {
        let normalized = normalize_key(key)?;
        let encrypted = self.encrypt(value.as_bytes())?;
        let mut file = self.load_file()?;
        file.secrets.insert(normalized, encrypted);
        self.save_file(&file)
    }

    pub fn get_secret(&self, key: &str) -> Result<Option<String>, io::Error> {
        let normalized = normalize_key(key)?;
        let file = self.load_file()?;
        let Some(secret) = file.secrets.get(&normalized) else {
            return Ok(None);
        };
        let plaintext = self.decrypt(secret)?;
        let as_utf8 = String::from_utf8(plaintext).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("vault secret is not valid UTF-8: {err}"),
            )
        })?;
        Ok(Some(as_utf8))
    }

    pub fn remove_secret(&self, key: &str) -> Result<(), io::Error> {
        let normalized = normalize_key(key)?;
        let mut file = self.load_file()?;
        file.secrets.remove(&normalized);
        self.save_file(&file)
    }

    fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedSecret, io::Error> {
        let mut nonce = [0u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| io::Error::other("failed to generate vault nonce"))?;

        let unbound = aead::UnboundKey::new(&aead::CHACHA20_POLY1305, &self.key_bytes)
            .map_err(|_| io::Error::other("failed to initialize vault cipher"))?;
        let key = aead::LessSafeKey::new(unbound);

        let mut in_out = plaintext.to_vec();
        key.seal_in_place_append_tag(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::empty(),
            &mut in_out,
        )
        .map_err(|_| io::Error::other("failed to encrypt vault secret"))?;

        Ok(EncryptedSecret {
            nonce_b64: STANDARD_NO_PAD.encode(nonce),
            ciphertext_b64: STANDARD_NO_PAD.encode(in_out),
        })
    }

    fn decrypt(&self, encrypted: &EncryptedSecret) -> Result<Vec<u8>, io::Error> {
        let nonce_vec = STANDARD_NO_PAD
            .decode(encrypted.nonce_b64.as_bytes())
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid vault nonce encoding: {err}"),
                )
            })?;
        if nonce_vec.len() != NONCE_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid vault nonce length",
            ));
        }

        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&nonce_vec);

        let mut in_out = STANDARD_NO_PAD
            .decode(encrypted.ciphertext_b64.as_bytes())
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid vault ciphertext encoding: {err}"),
                )
            })?;

        let unbound = aead::UnboundKey::new(&aead::CHACHA20_POLY1305, &self.key_bytes)
            .map_err(|_| io::Error::other("failed to initialize vault cipher"))?;
        let key = aead::LessSafeKey::new(unbound);

        let plain = key
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::empty(),
                &mut in_out,
            )
            .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "vault decrypt failed"))?;
        Ok(plain.to_vec())
    }

    fn load_file(&self) -> Result<VaultFile, io::Error> {
        if !self.data_path.exists() {
            return Ok(VaultFile::default());
        }
        let raw = fs::read_to_string(&self.data_path)?;
        serde_json::from_str::<VaultFile>(&raw).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid vault data file format: {err}"),
            )
        })
    }

    fn save_file(&self, file: &VaultFile) -> Result<(), io::Error> {
        let raw = serde_json::to_string_pretty(file).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("cannot serialize vault data: {err}"),
            )
        })?;
        fs::write(&self.data_path, raw)?;
        Ok(())
    }
}

fn normalize_key(value: &str) -> Result<String, io::Error> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "vault key cannot be empty",
        ));
    }
    Ok(normalized)
}

fn read_or_create_key(path: &Path) -> Result<[u8; 32], io::Error> {
    if path.exists() {
        let encoded = fs::read_to_string(path)?;
        let decoded = STANDARD_NO_PAD
            .decode(encoded.trim().as_bytes())
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid vault key encoding: {err}"),
                )
            })?;
        if decoded.len() != 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "vault key has invalid length",
            ));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&decoded);
        return Ok(key);
    }

    let mut key = [0u8; 32];
    SystemRandom::new()
        .fill(&mut key)
        .map_err(|_| io::Error::other("failed to generate vault key"))?;
    fs::write(path, STANDARD_NO_PAD.encode(key))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)?;
    }

    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_remove_roundtrip() {
        let dir = std::env::temp_dir().join("agent_secrets_vault_roundtrip");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let vault = SecretsVault::open(&dir).unwrap();
        vault
            .set_secret("agent.api_key", "super-secret-token")
            .unwrap();

        let value = vault.get_secret("agent.api_key").unwrap();
        assert_eq!(value.as_deref(), Some("super-secret-token"));

        vault.remove_secret("agent.api_key").unwrap();
        let removed = vault.get_secret("agent.api_key").unwrap();
        assert!(removed.is_none());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn vault_file_does_not_store_plaintext() {
        let dir = std::env::temp_dir().join("agent_secrets_vault_ciphertext");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let vault = SecretsVault::open(&dir).unwrap();
        vault
            .set_secret("agent.api_key", "plain-text-value")
            .unwrap();

        let raw = fs::read_to_string(dir.join(DATA_FILE_NAME)).unwrap();
        assert!(!raw.contains("plain-text-value"));
        assert!(dir.join(KEY_FILE_NAME).exists());

        fs::remove_dir_all(&dir).unwrap();
    }
}
