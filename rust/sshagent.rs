//! ssh-agent protocol: client for any agent (OpenSSH's or our own), plus a
//! minimal built-in agent so `session_key = "ssh-agent"` works on hosts with
//! no openssh at all.
//!
//! Why an agent at all: the session-10 custody note. A seed file on disk can
//! be read by anyone with filesystem access — including the historian — for
//! the whole life of the session. A seed held by an agent process exists only
//! in that process's memory: the harness asks the agent for *signatures*,
//! never for the key. Shred becomes REMOVE_IDENTITY, and a crashed session's
//! seed dies with its agent. This narrows the custody imbalance; it does not
//! erase it (whoever owns the machine owns its memory) — recorded honestly,
//! per the provenance notes.
//!
//! Wire format (draft-miller-ssh-agent): u32 BE frames; inside, u32-length
//! strings. Only ed25519 and only five messages are needed here.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ed25519_dalek::{Signer, SigningKey};

const SSH_AGENT_FAILURE: u8 = 5;
const SSH_AGENT_SUCCESS: u8 = 6;
const SSH_AGENTC_REQUEST_IDENTITIES: u8 = 11;
const SSH_AGENT_IDENTITIES_ANSWER: u8 = 12;
const SSH_AGENTC_SIGN_REQUEST: u8 = 13;
const SSH_AGENT_SIGN_RESPONSE: u8 = 14;
const SSH_AGENTC_ADD_IDENTITY: u8 = 17;
const SSH_AGENTC_REMOVE_IDENTITY: u8 = 18;

const KEY_TYPE: &[u8] = b"ssh-ed25519";

// ---- wire helpers -----------------------------------------------------------

fn put_str(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(&(s.len() as u32).to_be_bytes());
    out.extend_from_slice(s);
}

fn get_str<'a>(buf: &'a [u8], pos: &mut usize) -> Result<&'a [u8], String> {
    if buf.len() < *pos + 4 {
        return Err("agent reply truncated (length)".into());
    }
    let n = u32::from_be_bytes(buf[*pos..*pos + 4].try_into().unwrap()) as usize;
    *pos += 4;
    if buf.len() < *pos + n {
        return Err("agent reply truncated (string)".into());
    }
    let s = &buf[*pos..*pos + n];
    *pos += n;
    Ok(s)
}

/// The public-key blob an agent indexes ed25519 keys by.
pub fn key_blob(public: &[u8; 32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(51);
    put_str(&mut b, KEY_TYPE);
    put_str(&mut b, public);
    b
}

fn roundtrip(sock: &Path, msg: &[u8]) -> Result<Vec<u8>, String> {
    let mut s = UnixStream::connect(sock)
        .map_err(|e| format!("no agent at {}: {e}", sock.display()))?;
    s.write_all(&(msg.len() as u32).to_be_bytes())
        .and_then(|()| s.write_all(msg))
        .map_err(|e| format!("agent write: {e}"))?;
    let mut len = [0u8; 4];
    s.read_exact(&mut len).map_err(|e| format!("agent read: {e}"))?;
    let mut reply = vec![0u8; u32::from_be_bytes(len) as usize];
    s.read_exact(&mut reply).map_err(|e| format!("agent read: {e}"))?;
    Ok(reply)
}

// ---- client -----------------------------------------------------------------

/// Where the session's agent lives: an explicit override, the standard
/// OpenSSH variable, or the harness-local socket beside the door.
pub fn socket_path(comms_dir: &Path) -> PathBuf {
    if let Ok(p) = std::env::var("COMMS_AGENT_SOCK") {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var("SSH_AUTH_SOCK") {
        return PathBuf::from(p);
    }
    comms_dir.join("agent.sock")
}

/// Hand a fresh seed to the agent and forget it. The comment travels with the
/// key so `list` output stays attributable.
pub fn add_identity(sock: &Path, sk: &SigningKey, comment: &str) -> Result<(), String> {
    let public = *sk.verifying_key().as_bytes();
    let mut priv_blob = Vec::with_capacity(64);
    priv_blob.extend_from_slice(&sk.to_bytes());
    priv_blob.extend_from_slice(&public);
    let mut msg = vec![SSH_AGENTC_ADD_IDENTITY];
    put_str(&mut msg, KEY_TYPE);
    put_str(&mut msg, &public);
    put_str(&mut msg, &priv_blob);
    put_str(&mut msg, comment.as_bytes());
    match roundtrip(sock, &msg)?.first() {
        Some(&SSH_AGENT_SUCCESS) => Ok(()),
        _ => Err("agent refused the identity".into()),
    }
}

/// Every ed25519 identity the agent holds: (public key, comment).
pub fn list(sock: &Path) -> Result<Vec<([u8; 32], String)>, String> {
    let reply = roundtrip(sock, &[SSH_AGENTC_REQUEST_IDENTITIES])?;
    if reply.first() != Some(&SSH_AGENT_IDENTITIES_ANSWER) {
        return Err("agent did not answer the identity request".into());
    }
    let mut pos = 1;
    if reply.len() < pos + 4 {
        return Err("agent reply truncated (count)".into());
    }
    let count = u32::from_be_bytes(reply[pos..pos + 4].try_into().unwrap());
    pos += 4;
    let mut out = Vec::new();
    for _ in 0..count {
        let blob = get_str(&reply, &mut pos)?.to_vec();
        let comment = String::from_utf8_lossy(get_str(&reply, &mut pos)?).into_owned();
        let mut bp = 0;
        if get_str(&blob, &mut bp).ok().map(|t| t == KEY_TYPE).unwrap_or(false) {
            if let Ok(pk) = get_str(&blob, &mut bp) {
                if let Ok(public) = <[u8; 32]>::try_from(pk) {
                    out.push((public, comment));
                }
            }
        }
    }
    Ok(out)
}

/// Whether the agent holds this exact public key.
pub fn holds(sock: &Path, public: &[u8; 32]) -> bool {
    list(sock)
        .map(|ids| ids.iter().any(|(p, _)| p == public))
        .unwrap_or(false)
}

/// Raw ed25519 signature over `data` by the agent-held key. The agent signs
/// exactly the bytes given; the reply's blob unwraps to the 64-byte signature.
pub fn sign(sock: &Path, public: &[u8; 32], data: &[u8]) -> Result<[u8; 64], String> {
    let mut msg = vec![SSH_AGENTC_SIGN_REQUEST];
    put_str(&mut msg, &key_blob(public));
    put_str(&mut msg, data);
    msg.extend_from_slice(&0u32.to_be_bytes()); // flags
    let reply = roundtrip(sock, &msg)?;
    if reply.first() != Some(&SSH_AGENT_SIGN_RESPONSE) {
        return Err(
            "agent would not sign (key absent? session shredded or never minted here)".into(),
        );
    }
    let mut pos = 1;
    let sig_blob = get_str(&reply, &mut pos)?;
    let mut sp = 0;
    let t = get_str(sig_blob, &mut sp)?;
    if t != KEY_TYPE {
        return Err(format!(
            "agent signed with {}, not ed25519",
            String::from_utf8_lossy(t)
        ));
    }
    let sig = get_str(sig_blob, &mut sp)?;
    <[u8; 64]>::try_from(sig).map_err(|_| "agent signature is not 64 bytes".into())
}

/// Destroy the agent's copy of this key: the shred verb of agent mode.
pub fn remove_identity(sock: &Path, public: &[u8; 32]) -> Result<(), String> {
    let mut msg = vec![SSH_AGENTC_REMOVE_IDENTITY];
    put_str(&mut msg, &key_blob(public));
    match roundtrip(sock, &msg)?.first() {
        Some(&SSH_AGENT_SUCCESS) => Ok(()),
        _ => Err("agent refused to remove the identity".into()),
    }
}

// ---- built-in agent ---------------------------------------------------------

/// A minimal in-memory agent for hosts without openssh. Seeds live only in
/// this process; the socket is owner-only. Runs until killed — the harness
/// starts it in the background at session open and its death is the shred of
/// last resort.
pub fn serve(sock: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(sock);
    let listener =
        UnixListener::bind(sock).map_err(|e| format!("cannot bind {}: {e}", sock.display()))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(sock, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("cannot restrict {}: {e}", sock.display()))?;
    let keys: Mutex<HashMap<Vec<u8>, (SigningKey, String)>> = Mutex::new(HashMap::new());
    for stream in listener.incoming() {
        let Ok(mut s) = stream else { continue };
        loop {
            let mut len = [0u8; 4];
            if s.read_exact(&mut len).is_err() {
                break;
            }
            let mut msg = vec![0u8; u32::from_be_bytes(len) as usize];
            if s.read_exact(&mut msg).is_err() {
                break;
            }
            let reply = handle(&keys, &msg);
            if s.write_all(&(reply.len() as u32).to_be_bytes()).is_err()
                || s.write_all(&reply).is_err()
            {
                break;
            }
        }
    }
    Ok(())
}

fn handle(keys: &Mutex<HashMap<Vec<u8>, (SigningKey, String)>>, msg: &[u8]) -> Vec<u8> {
    let fail = vec![SSH_AGENT_FAILURE];
    let Some(&code) = msg.first() else { return fail };
    let body = &msg[1..];
    match code {
        SSH_AGENTC_ADD_IDENTITY => (|| -> Result<Vec<u8>, String> {
            let mut pos = 0;
            let t = get_str(body, &mut pos)?;
            if t != KEY_TYPE {
                return Err("only ed25519".into());
            }
            let public = get_str(body, &mut pos)?.to_vec();
            let priv_blob = get_str(body, &mut pos)?;
            let comment = String::from_utf8_lossy(get_str(body, &mut pos)?).into_owned();
            let seed: [u8; 32] = priv_blob
                .get(..32)
                .and_then(|s| s.try_into().ok())
                .ok_or("short private blob")?;
            let sk = SigningKey::from_bytes(&seed);
            if sk.verifying_key().as_bytes() != public.as_slice() {
                return Err("private half does not derive the public half".into());
            }
            let pk32: [u8; 32] = public.as_slice().try_into().map_err(|_| "bad pubkey")?;
            keys.lock().unwrap().insert(key_blob(&pk32), (sk, comment));
            Ok(vec![SSH_AGENT_SUCCESS])
        })()
        .unwrap_or(fail),
        SSH_AGENTC_REQUEST_IDENTITIES => {
            let held = keys.lock().unwrap();
            let mut out = vec![SSH_AGENT_IDENTITIES_ANSWER];
            out.extend_from_slice(&(held.len() as u32).to_be_bytes());
            for (blob, (_, comment)) in held.iter() {
                put_str(&mut out, blob);
                put_str(&mut out, comment.as_bytes());
            }
            out
        }
        SSH_AGENTC_SIGN_REQUEST => (|| -> Result<Vec<u8>, String> {
            let mut pos = 0;
            let blob = get_str(body, &mut pos)?.to_vec();
            let data = get_str(body, &mut pos)?;
            let held = keys.lock().unwrap();
            let (sk, _) = held.get(&blob).ok_or("unknown key")?;
            let sig = sk.sign(data).to_bytes();
            let mut sig_blob = Vec::new();
            put_str(&mut sig_blob, KEY_TYPE);
            put_str(&mut sig_blob, &sig);
            let mut out = vec![SSH_AGENT_SIGN_RESPONSE];
            put_str(&mut out, &sig_blob);
            Ok(out)
        })()
        .unwrap_or(fail),
        SSH_AGENTC_REMOVE_IDENTITY => (|| -> Result<Vec<u8>, String> {
            let mut pos = 0;
            let blob = get_str(body, &mut pos)?.to_vec();
            keys.lock()
                .unwrap()
                .remove(&blob)
                .map(|_| vec![SSH_AGENT_SUCCESS])
                .ok_or_else(|| "unknown key".into())
        })()
        .unwrap_or(fail),
        _ => fail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Verifier;

    #[test]
    fn add_list_sign_remove_round_trip() {
        let dir = std::env::temp_dir().join(format!("comms-agent-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join("test-agent.sock");
        let s2 = sock.clone();
        std::thread::spawn(move || {
            let _ = serve(&s2);
        });
        for _ in 0..50 {
            if sock.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let public = *sk.verifying_key().as_bytes();
        add_identity(&sock, &sk, "comms.steward:test").unwrap();
        assert!(holds(&sock, &public));

        let payload = b"the agent signs exactly these bytes";
        let sig = sign(&sock, &public, payload).unwrap();
        sk.verifying_key()
            .verify(payload, &ed25519_dalek::Signature::from_bytes(&sig))
            .expect("agent signature must verify against the raw public key");

        remove_identity(&sock, &public).unwrap();
        assert!(!holds(&sock, &public));
        assert!(
            sign(&sock, &public, payload).is_err(),
            "a removed key must not sign — dead sessions cannot speak"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
