//! comms-verify: the portable sneakernet kit.
//!
//! One static binary for the whole offline loop — pack, seal, inspect, verify,
//! extract — needing no Python or Cargo on the courier machine. Every verify
//! function is layer 2/3 only (verified + resolvable, per A1.4): a pass means
//! "the math holds," never "trust this." Trust stays a human judgment.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::process;

use comms_core::bundle::{
    build_seal, inspect_bundle, make_bundle, media_key, parse_attestation, parse_bundle,
    verify_seal, Bundle, InspectReport,
};
use comms_core::personal_steward_id;
use comms_core::steward::Attestation;
use ed25519_dalek::SigningKey;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let argv = &args[1..];
    match argv.first().map(String::as_str) {
        None => {
            usage();
            process::exit(1);
        }
        Some("-h") | Some("--help") => {
            usage();
            process::exit(0);
        }
        Some("verify") => cmd_verify(&argv[1..]),
        Some("inspect") => cmd_inspect(&argv[1..]),
        Some("seal") => cmd_seal(&argv[1..]),
        Some("pack") => cmd_pack(&argv[1..]),
        Some("extract") => cmd_extract(&argv[1..]),
        Some("mint") => cmd_mint(&argv[1..]),
        // Back-compat: `comms-verify <bundle.cbor>` (no subcommand) == verify.
        Some(_) => cmd_verify(argv),
    }
}

fn usage() {
    eprintln!("usage: comms-verify <command> [args]");
    eprintln!();
    eprintln!("The portable sneakernet kit for Comms Attest 1.0 bundles.");
    eprintln!();
    eprintln!("commands:");
    eprintln!("  verify  <bundle>                 check the A1.8 integrity seal (default)");
    eprintln!("  inspect <bundle> [--json]        verify every member on its own terms");
    eprintln!("  seal    <bundle> --key <k> [--out <p>] [--description S] [--*-at T]");
    eprintln!("  pack    --out <bundle> <att.cbor|dir>... [--media F]... [--seal --key <k>]");
    eprintln!("  extract <bundle> --out <dir>     write members and media to files");
    eprintln!("  mint    --out <key.json> [--label L]   generate a steward key for sealing");
    eprintln!();
    eprintln!("A 'valid' result is layer 2/3 (verified + resolvable). Trust is yours.");
}

// ---- shared helpers --------------------------------------------------------

fn die(msg: impl AsRef<str>) -> ! {
    eprintln!("error: {}", msg.as_ref());
    process::exit(1);
}

fn read_file(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| die(format!("cannot read {path}: {e}")))
}

fn read_bundle(path: &str) -> Bundle {
    parse_bundle(&read_file(path)).unwrap_or_else(|e| die(format!("{path}: {e}")))
}

/// Tiny option parser. `--flag value` for value options, repeated for the
/// multi option `--media`, bare for the boolean flags `--seal` / `--json`.
#[derive(Default)]
struct Opts {
    positionals: Vec<String>,
    values: HashMap<String, String>,
    media: Vec<String>,
    flags: HashSet<String>,
}

fn parse_opts(args: &[String]) -> Opts {
    const BOOLS: &[&str] = &["--seal", "--json"];
    let mut o = Opts::default();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a.starts_with("--") {
            if BOOLS.contains(&a.as_str()) {
                o.flags.insert(a.clone());
            } else {
                i += 1;
                let val = args
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| die(format!("{a} needs a value")));
                if a == "--media" {
                    o.media.push(val);
                } else {
                    o.values.insert(a.clone(), val);
                }
            }
        } else {
            o.positionals.push(a.clone());
        }
        i += 1;
    }
    o
}

impl Opts {
    fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
    fn require(&self, key: &str) -> &str {
        self.get(key).unwrap_or_else(|| die(format!("missing required {key}")))
    }
    fn has(&self, flag: &str) -> bool {
        self.flags.contains(flag)
    }
}

fn load_key(path: &str) -> SigningKey {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| die(format!("cannot read key {path}: {e}")));
    let v: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| die(format!("key {path} is not JSON: {e}")));
    let seed_b58 = v["seed_b58"]
        .as_str()
        .unwrap_or_else(|| die(format!("key {path} has no seed_b58 field")));
    let seed = bs58::decode(seed_b58)
        .into_vec()
        .unwrap_or_else(|e| die(format!("seed_b58 is not base58: {e}")));
    let seed: [u8; 32] = seed
        .as_slice()
        .try_into()
        .unwrap_or_else(|_| die("steward seed is not 32 bytes"));
    SigningKey::from_bytes(&seed)
}

fn os_random_32() -> [u8; 32] {
    let mut f =
        std::fs::File::open("/dev/urandom").unwrap_or_else(|e| die(format!("/dev/urandom: {e}")));
    let mut buf = [0u8; 32];
    f.read_exact(&mut buf).unwrap_or_else(|e| die(format!("reading randomness: {e}")));
    buf
}

/// RFC 3339 UTC, second precision, `Z` suffix (the A1.6 canonical timestamp).
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // civil_from_days (Howard Hinnant): epoch day 0 == 1970-01-01.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Pull --created-at/--issued-at/--signed-at, each defaulting to now.
fn timestamps(o: &Opts) -> (String, String, String) {
    let now = now_rfc3339();
    (
        o.get("--created-at").map(str::to_owned).unwrap_or_else(|| now.clone()),
        o.get("--issued-at").map(str::to_owned).unwrap_or_else(|| now.clone()),
        o.get("--signed-at").map(str::to_owned).unwrap_or(now),
    )
}

// ---- verify ----------------------------------------------------------------

fn cmd_verify(args: &[String]) {
    let o = parse_opts(args);
    let path = o
        .positionals
        .first()
        .map(String::as_str)
        .unwrap_or_else(|| die("usage: comms-verify verify <bundle.cbor>"));
    let bundle = read_bundle(path);

    let seal_ids: HashSet<String> = bundle.members().iter().map(Attestation::id).collect();
    let members = seal_ids.len();
    let total = bundle.attestations.len();
    println!(
        "bundle: {total} attestation{} ({members} member{}, {} seal{})",
        plural(total),
        plural(members),
        total - members,
        plural(total - members),
    );
    if !bundle.media.is_empty() {
        println!("media: {} blob{}", bundle.media.len(), plural(bundle.media.len()));
    }

    let report = verify_seal(&bundle);
    if let Some(by) = &report.sealed_by {
        println!("sealed by: {by}");
    }
    if report.ok {
        println!("seal: ok");
        process::exit(0);
    }
    println!("seal: FAIL");
    if !report.signature_ok {
        println!("  signature: invalid");
    }
    if !report.hash_ok {
        println!("  bundle hash: mismatch");
    }
    for id in &report.missing {
        println!("  missing member: {id}");
    }
    for id in &report.extra {
        println!("  extra member: {id}");
    }
    process::exit(1);
}

// ---- inspect ---------------------------------------------------------------

fn cmd_inspect(args: &[String]) {
    let o = parse_opts(args);
    let path = o
        .positionals
        .first()
        .map(String::as_str)
        .unwrap_or_else(|| die("usage: comms-verify inspect <bundle.cbor> [--json]"));
    let bundle = read_bundle(path);
    let report = inspect_bundle(&bundle);

    if o.has("--json") {
        print!("{}", inspect_json(&report));
    } else {
        print_inspect(&report);
    }
    process::exit(if inspect_ok(&report) { 0 } else { 1 });
}

/// Everything verified: every member's signatures hold, media content matches,
/// and any seal present is valid.
fn inspect_ok(r: &InspectReport) -> bool {
    r.members.iter().all(|m| m.all_signatures_ok)
        && r.media.iter().all(|(_, ok)| *ok)
        && (r.seal.sealed_by.is_none() || r.seal.ok)
}

fn print_inspect(r: &InspectReport) {
    for m in &r.members {
        let tag = if m.is_seal { "  [A1.8 seal]" } else { "" };
        let mark = if m.all_signatures_ok { "ok" } else { "INVALID" };
        println!("{} ({}){tag}  signatures: {mark}", m.id, m.claim_type);
        for s in &m.signatures {
            let glyph = if s.ok { "✓" } else { "✗" };
            println!("    {glyph} {} by {} — {}", s.role, s.by, s.detail);
        }
        for r in &m.refs {
            let state = if r.resolves_in_bundle { "resolved" } else { "awaiting context" };
            println!("    ref {} -> {} [{}]", r.role, r.id, state);
        }
    }
    if !r.media.is_empty() {
        println!("media:");
        for (k, ok) in &r.media {
            println!("    {} {}", if *ok { "✓" } else { "✗ content mismatch" }, k);
        }
    }
    match &r.seal.sealed_by {
        Some(by) if r.seal.ok => println!("seal: ok (sealed by {by})"),
        Some(by) => println!("seal: FAIL (sealed by {by})"),
        None => println!("seal: none present"),
    }
}

fn inspect_json(r: &InspectReport) -> String {
    let members: Vec<_> = r
        .members
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "claim_type": m.claim_type,
                "is_seal": m.is_seal,
                "all_signatures_ok": m.all_signatures_ok,
                "signatures": m.signatures.iter().map(|s| serde_json::json!({
                    "by": s.by, "role": s.role, "alg": s.alg, "ok": s.ok, "detail": s.detail,
                })).collect::<Vec<_>>(),
                "refs": m.refs.iter().map(|rf| serde_json::json!({
                    "role": rf.role, "id": rf.id, "resolves_in_bundle": rf.resolves_in_bundle,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let out = serde_json::json!({
        "members": members,
        "media": r.media.iter().map(|(k, ok)| serde_json::json!({"key": k, "ok": ok}))
            .collect::<Vec<_>>(),
        "seal": {
            "present": r.seal.sealed_by.is_some(),
            "ok": r.seal.ok,
            "sealed_by": r.seal.sealed_by,
            "signature_ok": r.seal.signature_ok,
            "hash_ok": r.seal.hash_ok,
            "members_match": r.seal.members_match,
            "missing": r.seal.missing,
            "extra": r.seal.extra,
        },
        "all_ok": inspect_ok(r),
    });
    format!("{}\n", serde_json::to_string_pretty(&out).unwrap())
}

// ---- seal ------------------------------------------------------------------

fn cmd_seal(args: &[String]) {
    let o = parse_opts(args);
    let path = o
        .positionals
        .first()
        .map(String::as_str)
        .unwrap_or_else(|| die("usage: comms-verify seal <bundle.cbor> --key <key.json>"));
    let bundle = read_bundle(path);
    if bundle.is_sealed() {
        die("bundle already carries a seal; refusing to add a second");
    }
    let sk = load_key(o.require("--key"));
    let (created_at, issued_at, signed_at) = timestamps(&o);
    let description = o.get("--description").unwrap_or("");

    let members = bundle.members();
    let seal = build_seal(&members, &sk, description, &created_at, &issued_at, &signed_at);
    let mut attestations = bundle.attestations.clone();
    attestations.push(seal);
    let sealed = Bundle { attestations, media: bundle.media, manifest: bundle.manifest };

    let out = o.get("--out").unwrap_or(path);
    write_out(out, &sealed.to_cbor());
    println!("sealed {} member{} -> {out}", members.len(), plural(members.len()));
    println!("sealed by: {}", personal_steward_id(sk.verifying_key().as_bytes()));
}

// ---- pack ------------------------------------------------------------------

fn cmd_pack(args: &[String]) {
    let o = parse_opts(args);
    let out = o.require("--out");
    if o.positionals.is_empty() {
        die("pack needs at least one attestation file or directory");
    }

    let mut members = Vec::new();
    for p in &o.positionals {
        for file in expand_cbor_paths(p) {
            let att = parse_attestation(&read_file(&file))
                .unwrap_or_else(|e| die(format!("{file}: {e}")));
            members.push(att);
        }
    }

    let mut media = HashMap::new();
    for f in &o.media {
        let blob = read_file(f);
        media.insert(media_key(&blob), blob);
    }

    let (created_at, issued_at, signed_at) = timestamps(&o);
    let description = o.get("--description").unwrap_or("");
    let sealer = if o.has("--seal") {
        Some(load_key(o.require("--key")))
    } else {
        if o.get("--key").is_some() {
            die("--key given without --seal; did you mean to seal?");
        }
        None
    };

    let bundle = make_bundle(
        members.clone(),
        media,
        sealer.as_ref(),
        description,
        &created_at,
        &issued_at,
        &signed_at,
    );
    write_out(out, &bundle.to_cbor());
    println!(
        "packed {} member{} ({}) -> {out}",
        members.len(),
        plural(members.len()),
        if sealer.is_some() { "sealed" } else { "unsealed" },
    );
}

/// A path is either a `.cbor` file or a directory; a directory contributes its
/// immediate `.cbor` children (sorted, for deterministic member order).
fn expand_cbor_paths(path: &str) -> Vec<String> {
    let meta = std::fs::metadata(path).unwrap_or_else(|e| die(format!("{path}: {e}")));
    if meta.is_dir() {
        let mut out: Vec<String> = std::fs::read_dir(path)
            .unwrap_or_else(|e| die(format!("{path}: {e}")))
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|x| x == "cbor").unwrap_or(false))
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        out.sort();
        out
    } else {
        vec![path.to_owned()]
    }
}

// ---- extract ---------------------------------------------------------------

fn cmd_extract(args: &[String]) {
    let o = parse_opts(args);
    let path = o
        .positionals
        .first()
        .map(String::as_str)
        .unwrap_or_else(|| die("usage: comms-verify extract <bundle.cbor> --out <dir>"));
    let dir = o.require("--out");
    let bundle = read_bundle(path);
    std::fs::create_dir_all(dir).unwrap_or_else(|e| die(format!("{dir}: {e}")));

    for att in &bundle.attestations {
        // The attestation id is a stable, collision-free filename stem.
        let file = format!("{dir}/{}.cbor", att.id());
        write_out(&file, &att.to_cbor());
    }
    for (key, blob) in &bundle.media {
        write_out(&format!("{dir}/{key}"), blob);
    }
    println!(
        "extracted {} attestation{} and {} media blob{} -> {dir}",
        bundle.attestations.len(),
        plural(bundle.attestations.len()),
        bundle.media.len(),
        plural(bundle.media.len()),
    );
}

// ---- mint ------------------------------------------------------------------

fn cmd_mint(args: &[String]) {
    let o = parse_opts(args);
    let out = o.require("--out");
    let label = o.get("--label").unwrap_or("");
    let seed = os_random_32();
    let sk = SigningKey::from_bytes(&seed);
    let id = personal_steward_id(sk.verifying_key().as_bytes());

    let json = serde_json::json!({ "seed_b58": bs58::encode(seed).into_string(), "label": label });
    write_out(out, serde_json::to_string(&json).unwrap().as_bytes());
    restrict_permissions(out);
    println!("minted steward {id}");
    println!("key written to {out} (mode 0600); keep the seed secret");
}

fn restrict_permissions(path: &str) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
}

// ---- misc ------------------------------------------------------------------

fn write_out(path: &str, data: &[u8]) {
    std::fs::write(path, data).unwrap_or_else(|e| die(format!("cannot write {path}: {e}")));
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
