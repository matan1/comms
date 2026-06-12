//! Minimal deterministic CBOR — the RFC 8949 §4.2.1 subset Comms actually uses.
//!
//! Canonical **by construction**: definite lengths only, shortest-form heads,
//! map keys sorted by bytewise comparison of their canonical encodings at
//! encode time. No floats, no tags, no negative integers — the protocol
//! defines none, so the encoder cannot emit them and the decoder rejects them.
//!
//! The decoder is strict: it accepts only bytes the encoder could have
//! produced (shortest-form heads enforced, trailing bytes rejected). This
//! makes `encode(decode(x)) == x` a real conformance check rather than a
//! tautology: if it holds, both sides agree on canonical form.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    U64(u64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
}

impl Value {
    pub fn text(s: &str) -> Value {
        Value::Text(s.to_owned())
    }

    /// Map field lookup by text key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries
                .iter()
                .find(|(k, _)| matches!(k, Value::Text(t) if t == key))
                .map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::U64(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CborError {
    Truncated,
    /// Major type or simple value outside the protocol subset (floats, tags,
    /// negative integers, indefinite lengths).
    Unsupported(u8),
    /// Valid CBOR, but not shortest-form / deterministic.
    NonCanonical,
    TrailingBytes,
    BadUtf8,
}

impl std::fmt::Display for CborError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CborError::Truncated => write!(f, "truncated CBOR"),
            CborError::Unsupported(b) => write!(f, "unsupported CBOR item (initial byte {b:#04x})"),
            CborError::NonCanonical => write!(f, "non-canonical CBOR encoding"),
            CborError::TrailingBytes => write!(f, "trailing bytes after CBOR item"),
            CborError::BadUtf8 => write!(f, "text string is not valid UTF-8"),
        }
    }
}

impl std::error::Error for CborError {}

pub fn encode(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    enc(v, &mut out);
    out
}

fn head(major: u8, arg: u64, out: &mut Vec<u8>) {
    match arg {
        0..=23 => out.push(major << 5 | arg as u8),
        24..=0xff => {
            out.push(major << 5 | 24);
            out.push(arg as u8);
        }
        0x100..=0xffff => {
            out.push(major << 5 | 25);
            out.extend_from_slice(&(arg as u16).to_be_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            out.push(major << 5 | 26);
            out.extend_from_slice(&(arg as u32).to_be_bytes());
        }
        _ => {
            out.push(major << 5 | 27);
            out.extend_from_slice(&arg.to_be_bytes());
        }
    }
}

fn enc(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::U64(n) => head(0, *n, out),
        Value::Bytes(b) => {
            head(2, b.len() as u64, out);
            out.extend_from_slice(b);
        }
        Value::Text(t) => {
            head(3, t.len() as u64, out);
            out.extend_from_slice(t.as_bytes());
        }
        Value::Array(a) => {
            head(4, a.len() as u64, out);
            for item in a {
                enc(item, out);
            }
        }
        Value::Map(entries) => {
            // Sort by bytewise comparison of canonical key encodings (RFC 8949).
            let mut encoded: Vec<(Vec<u8>, Vec<u8>)> = entries
                .iter()
                .map(|(k, v)| (encode(k), encode(v)))
                .collect();
            encoded.sort_by(|a, b| a.0.cmp(&b.0));
            head(5, encoded.len() as u64, out);
            for (k, v) in encoded {
                out.extend_from_slice(&k);
                out.extend_from_slice(&v);
            }
        }
    }
}

pub fn decode(data: &[u8]) -> Result<Value, CborError> {
    let mut pos = 0usize;
    let v = dec(data, &mut pos)?;
    if pos != data.len() {
        return Err(CborError::TrailingBytes);
    }
    Ok(v)
}

fn read_head(data: &[u8], pos: &mut usize) -> Result<(u8, u64), CborError> {
    let b = *data.get(*pos).ok_or(CborError::Truncated)?;
    *pos += 1;
    let major = b >> 5;
    let ai = b & 0x1f;
    let arg = match ai {
        0..=23 => u64::from(ai),
        24 => {
            let v = u64::from(*data.get(*pos).ok_or(CborError::Truncated)?);
            *pos += 1;
            if v < 24 {
                return Err(CborError::NonCanonical);
            }
            v
        }
        25 => {
            let s = data.get(*pos..*pos + 2).ok_or(CborError::Truncated)?;
            *pos += 2;
            let v = u64::from(u16::from_be_bytes([s[0], s[1]]));
            if v <= 0xff {
                return Err(CborError::NonCanonical);
            }
            v
        }
        26 => {
            let s = data.get(*pos..*pos + 4).ok_or(CborError::Truncated)?;
            *pos += 4;
            let v = u64::from(u32::from_be_bytes([s[0], s[1], s[2], s[3]]));
            if v <= 0xffff {
                return Err(CborError::NonCanonical);
            }
            v
        }
        27 => {
            let s = data.get(*pos..*pos + 8).ok_or(CborError::Truncated)?;
            *pos += 8;
            let v = u64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]);
            if v <= 0xffff_ffff {
                return Err(CborError::NonCanonical);
            }
            v
        }
        _ => return Err(CborError::Unsupported(b)),
    };
    Ok((major, arg))
}

fn take<'a>(data: &'a [u8], pos: &mut usize, len: u64) -> Result<&'a [u8], CborError> {
    let len = usize::try_from(len).map_err(|_| CborError::Truncated)?;
    let end = pos.checked_add(len).ok_or(CborError::Truncated)?;
    let s = data.get(*pos..end).ok_or(CborError::Truncated)?;
    *pos = end;
    Ok(s)
}

fn dec(data: &[u8], pos: &mut usize) -> Result<Value, CborError> {
    let start = *pos;
    let (major, arg) = read_head(data, pos)?;
    match major {
        0 => Ok(Value::U64(arg)),
        2 => Ok(Value::Bytes(take(data, pos, arg)?.to_vec())),
        3 => {
            let s = take(data, pos, arg)?;
            String::from_utf8(s.to_vec())
                .map(Value::Text)
                .map_err(|_| CborError::BadUtf8)
        }
        4 => {
            let mut a = Vec::new();
            for _ in 0..arg {
                a.push(dec(data, pos)?);
            }
            Ok(Value::Array(a))
        }
        5 => {
            let mut entries = Vec::new();
            for _ in 0..arg {
                let k = dec(data, pos)?;
                let v = dec(data, pos)?;
                entries.push((k, v));
            }
            Ok(Value::Map(entries))
        }
        _ => Err(CborError::Unsupported(data[start])),
    }
}
