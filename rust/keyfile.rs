//! Steward key files: the `{seed_b58, label}` JSON the Python toolkit writes
//! (`identity.py:Steward.save`). Factored out of the CLI so the rite engine
//! can mint and load the session key the same way `mint`/`seal` do.

use std::io::Read;
use std::path::Path;

use ed25519_dalek::SigningKey;

/// Load a signing key from a steward key file. `Err` carries a human message.
pub fn load(path: &Path) -> Result<SigningKey, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read key {}: {e}", path.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("key {} is not JSON: {e}", path.display()))?;
    let seed_b58 = v["seed_b58"]
        .as_str()
        .ok_or_else(|| format!("key {} has no seed_b58 field", path.display()))?;
    let seed = bs58::decode(seed_b58)
        .into_vec()
        .map_err(|e| format!("seed_b58 is not base58: {e}"))?;
    let seed: [u8; 32] = seed
        .as_slice()
        .try_into()
        .map_err(|_| "steward seed is not 32 bytes".to_owned())?;
    Ok(SigningKey::from_bytes(&seed))
}

/// Mint a fresh key and write it to `path` (mode 0600 on unix). Returns the key.
pub fn mint(path: &Path, label: &str) -> Result<SigningKey, String> {
    let seed = os_random_32()?;
    let sk = SigningKey::from_bytes(&seed);
    let json = serde_json::json!({ "seed_b58": bs58::encode(seed).into_string(), "label": label });
    std::fs::write(path, serde_json::to_string(&json).unwrap())
        .map_err(|e| format!("cannot write key {}: {e}", path.display()))?;
    restrict_permissions(path);
    Ok(sk)
}

/// Best-effort shred: overwrite the file's bytes before unlinking, so the seed
/// is not left lying in freed disk pages. Not a guarantee on copy-on-write
/// filesystems, but better than a bare unlink for a key meant to die at close.
pub fn shred(path: &Path) -> Result<(), String> {
    if let Ok(meta) = std::fs::metadata(path) {
        let zeros = vec![0u8; meta.len() as usize];
        let _ = std::fs::write(path, &zeros);
    }
    std::fs::remove_file(path).map_err(|e| format!("cannot remove key {}: {e}", path.display()))
}

fn os_random_32() -> Result<[u8; 32], String> {
    let mut f = std::fs::File::open("/dev/urandom").map_err(|e| format!("/dev/urandom: {e}"))?;
    let mut buf = [0u8; 32];
    f.read_exact(&mut buf).map_err(|e| format!("reading randomness: {e}"))?;
    Ok(buf)
}

fn restrict_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
}
