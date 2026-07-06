//! comms: the portable sneakernet kit.
//!
//! One static binary for the whole offline loop — pack, seal, inspect, verify,
//! extract — needing no Python or Cargo on the courier machine. Every verify
//! function is layer 2/3 only (verified + resolvable, per A1.4): a pass means
//! "the math holds," never "trust this." Trust stays a human judgment.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::process;

use comms_core::bundle::{
    author_general_claim, build_seal, content_report, inspect_bundle, make_bundle, media_key,
    parse_attestation, parse_bundle, verify_seal, BodyStatus, Bundle, ClaimSpec, ContentReport,
    InspectReport,
};
use comms_core::config::{self, HarnessConfig};
use comms_core::init::{install, profile_by_name, profile_names};
use comms_core::rites::{self, ExecInputs};
use comms_core::steward::Attestation;
use comms_core::{now_rfc3339, personal_steward_id};
use comms_core::vouch::{evaluate, judgment_receipt, Evaluation, Query, ENGINE};
use ed25519_dalek::SigningKey;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let argv = &args[1..];
    match argv.first().map(String::as_str) {
        None => {
            usage();
            process::exit(1);
        }
        Some("-h") | Some("--help") | Some("help") => {
            print!("{}", usage_text());
            process::exit(0);
        }
        Some("-V") | Some("--version") | Some("version") => {
            println!("comms {}", env!("CARGO_PKG_VERSION"));
            process::exit(0);
        }
        // `<command> --help` / `-h` prints that command's synopsis (and exits 0)
        // instead of being mistaken for a value-bearing option.
        Some(name) if argv[1..].iter().any(|a| a == "-h" || a == "--help") => {
            print!("{}", help_for(name));
            process::exit(0);
        }
        Some("init") => cmd_init(&argv[1..]),
        Some("attest") => cmd_attest(&argv[1..]),
        Some("status") => cmd_status(&argv[1..]),
        Some("next") => cmd_next(&argv[1..]),
        Some("verify") => cmd_verify(&argv[1..]),
        Some("inspect") => cmd_inspect(&argv[1..]),
        Some("seal") => cmd_seal(&argv[1..]),
        Some("pack") => cmd_pack(&argv[1..]),
        Some("extract") => cmd_extract(&argv[1..]),
        Some("intake") => cmd_intake(&argv[1..]),
        Some("audit") => cmd_audit(&argv[1..]),
        Some("mint") => cmd_mint(&argv[1..]),
        Some("waive") => cmd_waive(&argv[1..]),
        Some("sign") => cmd_sign(&argv[1..]),
        Some("finalize") => cmd_finalize(&argv[1..]),
        Some("vouch") => cmd_vouch(&argv[1..]),
        // Back-compat: `comms <bundle.cbor>` (no subcommand) == verify.
        Some(_) => cmd_verify(argv),
    }
}

fn usage() {
    eprint!("{}", usage_text());
}

fn usage_text() -> String {
    format!(
        "comms {} — the portable sneakernet kit for Comms Attest 1.0.\n\
         \n\
         usage: comms <command> [args]   (try `comms <command> --help`)\n\
         \n\
         commands:\n\
         \x20 init    [dir] [--profile P] [--dry-run] [--force]  install the .comms/ door\n\
         \x20 attest  --key <k> --about S --kind S --body <file|-> [--detach]\n\
         \x20         [--media-type T] [--language L] [--community C] [--occasion O]\n\
         \x20         [--role R] [--support ID]... [--out FILE]   author + sign a\n\
         \x20         general-claim/1 (--detach commits to the body without carrying it)\n\
         \x20 status  [dir] [--json]           where you are in the rite + next step\n\
         \x20 next    [dir] [--rite N] [--body F] [--about S]   perform the next step\n\
         \x20 verify  <bundle>                 check the A1.8 integrity seal (default)\n\
         \x20 inspect <bundle> [--json]        verify every member on its own terms\n\
         \x20 seal    <bundle> --key <k> [--out <p>] [--description S] [--*-at T]\n\
         \x20 pack    --out <bundle> <att.cbor|dir>... [--media F]... [--seal --key <k>]\n\
         \x20 extract <bundle> --out <dir>     write members and media to files\n\
         \x20 intake  <bundle|dir> [root] --key <k> [--legacy --provenance S]\n\
         \x20         archive custody: verify, ingest by id/hash, regenerate views,\n\
         \x20         attest custody (idempotent; archive profile)\n\
         \x20 audit   [root]                   re-derive every id and hash in custody;\n\
         \x20         mark drift, never delete (archive profile)\n\
         \x20 mint    --out <key.json> [--label L]   generate a steward key for sealing\n\
         \x20 waive   <type> [dir] --body <reason>   record that this session cannot\n\
         \x20         produce a required artifact (the gap becomes an attestation)\n\
         \x20 sign    --key <openssh|steward-key> [--pending DIR]   countersign staged\n\
         \x20         pending items (the counterparty's half of a rite)\n\
         \x20 finalize [--pending DIR] [--store DIR]  verify + move fully-signed\n\
         \x20         pending items into the store\n\
         \x20 vouch   <bundle> --policy ID --subject ID --purpose S --as-of T [--json]\n\
         \x20         [--community ID] [--receipt-out P --key K]\n\
         \n\
         A 'valid' result is layer 2/3 (verified + resolvable). Trust is yours.\n\
         Run with -V/--version for the version.\n",
        env!("CARGO_PKG_VERSION"),
    )
}

/// Per-command synopsis for `<command> --help`. Unknown names fall back to the
/// general usage (this includes the bare-bundle-path back-compat form).
fn help_for(cmd: &str) -> String {
    let synopsis = match cmd {
        "init" => "comms init [dir] [--profile default|continuity] [--dry-run] [--force]\n  Install or refresh the .comms/ harness door in a repo.\n",
        "attest" => "comms attest --key <k.json> --about S --kind S --body <file|-> [--detach] \\\n    [--media-type T] [--language L] [--community C] [--occasion O] [--role R] \\\n    [--support ID]... [--out FILE]\n  Author and sign a general-claim/1 from a content file (-> <id>.cbor).\n  --detach (A2): the claim commits to {body_b3, body_len} and the body bytes\n  stay out of the attestation — pair them back up in a bundle via pack --media.\n",
        "inspect" => "comms inspect <bundle> [--json]\n  Verify every member on its own terms (signatures, refs, media), and report\n  body status (verified | absent | mismatched) for detached bodies (A2.2).\n",
        "status" => "comms status [dir] [--json]\n  Report where you are in each rite and the exact next command.\n",
        "next" => "comms next [dir] [--rite N] [--body F] [--about S] [--kind K]\n  Perform the next pending step of a rite. attest steps need --body;\n  with no --rite, advances the rite you are currently in.\n",
        "verify" => "comms verify <bundle>\n  Check the A1.8 integrity seal (also the default for a bare bundle path).\n",
        "seal" => "comms seal <bundle> --key <k.json> [--out P] [--description S] [--created-at T] [--issued-at T] [--signed-at T]\n  Add an A1.8 integrity seal (signs the exact member set).\n",
        "pack" => "comms pack --out <bundle> [<att.cbor|dir>...] [--media F]... [--seal --key <k.json>] [--description S]\n  Gather attestations and/or media blobs into a bundle.\n",
        "extract" => "comms extract <bundle> --out <dir>\n  Write each member <id>.cbor and media blob to disk.\n",
        "intake" => "comms intake <bundle|file|dir> [archive-root] --key <custodian key> \\\n    [--legacy --provenance \"...\"]\n  The custodian's one verb at closeout (archive profile): verify the bundle's\n  seal/members/media, ingest members by id and bodies by hash (idempotent),\n  regenerate views/ for the affected sessions, and attest custody. --legacy\n  takes unattested material in as testimony, by hash, honestly labeled.\n",
        "audit" => "comms audit [archive-root]\n  Walk store/ and bodies/, re-derive every id and hash, and report intact /\n  absent / mismatched. Drift is marked, never deleted (preservation stance).\n",
        "mint" => "comms mint --out <key.json> [--label L]\n  Generate a steward key ({seed_b58, label} JSON, mode 0600).\n",
        "waive" => "comms waive <type> [dir] --body <reason file|->\n  Record a session-signed waiver: this session cannot produce a declared\n  `required_for` artifact, and says so on the record instead of being blocked.\n  Only rites with `allow_waivers = true` accept it at seal.\n",
        "sign" => "comms sign --key <path> [--pending DIR]\n  Countersign staged pending items (<name>.cbor + <name>.needs.json) with an\n  OpenSSH ed25519 key or a steward key file. Needs naming other keys are left\n  standing. Default DIR: .comms/pending or continuity/pending, whichever has\n  staged items.\n",
        "finalize" => "comms finalize [--pending DIR] [--store DIR]\n  Verify every fully-signed pending item and move it into the store under its\n  content id. Aborts loudly if anything is unsigned or invalid. Default store:\n  the sibling store/ of the pending directory.\n",
        "vouch" => "comms vouch <bundle> --policy ID --subject ID --purpose S --as-of T [--json] [--community ID] [--receipt-out P --key K]\n  Policy-relative evaluation: a viewer's judgment, not proof.\n",
        _ => return usage_text(),
    };
    synopsis.to_owned()
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
    support: Vec<String>,
    flags: HashSet<String>,
}

fn parse_opts(args: &[String]) -> Opts {
    const BOOLS: &[&str] =
        &["--seal", "--json", "--dry-run", "--force", "--detach", "--legacy", "-h", "--help"];
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
                match a.as_str() {
                    "--media" => o.media.push(val),
                    "--support" => o.support.push(val),
                    _ => {
                        o.values.insert(a.clone(), val);
                    }
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

fn hex_str(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn os_random_32() -> [u8; 32] {
    let mut f =
        std::fs::File::open("/dev/urandom").unwrap_or_else(|e| die(format!("/dev/urandom: {e}")));
    let mut buf = [0u8; 32];
    f.read_exact(&mut buf).unwrap_or_else(|e| die(format!("reading randomness: {e}")));
    buf
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

// ---- init ------------------------------------------------------------------

fn cmd_init(args: &[String]) {
    let o = parse_opts(args);
    let target = o.positionals.first().map(String::as_str).unwrap_or(".");
    let profile_name = o.get("--profile").unwrap_or("default");
    let profile = profile_by_name(profile_name).unwrap_or_else(|| {
        die(format!(
            "unknown profile '{profile_name}' (have: {})",
            profile_names().join(", ")
        ))
    });

    let dry = o.has("--dry-run");
    let force = o.has("--force");
    let steps = install(profile, std::path::Path::new(target), force, dry)
        .unwrap_or_else(|e| die(format!("init failed: {e}")));

    let where_ = if target == "." { "here".to_owned() } else { target.to_owned() };
    if dry {
        println!("init (dry run): profile '{}' into {where_}", profile.name);
    } else {
        println!("init: profile '{}' into {where_}", profile.name);
    }
    if steps.is_empty() {
        println!("  (nothing to do — door already present)");
    } else {
        for s in &steps {
            println!("{}", s.render());
        }
    }
    if !dry {
        println!("the door is the .comms/ dir; edit policy.md and comms.toml to fit your community.");
        println!("what's drivable today is in .comms/harness.md.");
        if profile.name == "default" {
            println!(
                "tip: other profiles available with --profile ({}).",
                profile_names().join(", ")
            );
        }
    }
}

// ---- attest ----------------------------------------------------------------

fn cmd_attest(args: &[String]) {
    let o = parse_opts(args);
    let sk = load_key(o.require("--key"));
    let about = o.require("--about");
    let kind = o.get("--kind").unwrap_or("testimony");
    let role = o.get("--role").unwrap_or("author");
    let media_type = o.get("--media-type").unwrap_or("text/plain;charset=utf-8");
    let language = o.get("--language").unwrap_or("zxx");

    let body_path = o.require("--body");
    let body = if body_path == "-" {
        read_stdin()
    } else {
        read_file(body_path)
    };

    let now = now_rfc3339();
    let issued_at = o.get("--issued-at").unwrap_or(now.as_str());
    let signed_at = o.get("--signed-at").unwrap_or(now.as_str());

    let detach = o.has("--detach");
    let spec = ClaimSpec {
        about,
        kind,
        body: &body,
        media_type,
        detach,
        support: &o.support,
        refs: &[],
        language,
        community: o.get("--community"),
        occasion: o.get("--occasion"),
        issued_at,
    };
    let att = author_general_claim(&spec, &sk, role, signed_at);
    let id = att.id();
    let out = o
        .get("--out")
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{id}.cbor"));

    write_out(&out, &att.to_cbor());
    println!("attested {id}");
    if detach {
        let b3 = blake3::hash(&body);
        println!(
            "  kind: {kind}   about: {about}   (detached: {} body bytes committed, {media_type})",
            body.len()
        );
        println!("  body_b3: {}   media key: z{}", b3.to_hex(), bs58::encode(b3.as_bytes()).into_string());
        println!("  the attestation commits to the body but does not carry it — keep the");
        println!("  body file where its custodian archives it (bundle it with --media).");
    } else {
        println!("  kind: {kind}   about: {about}   ({} body bytes, {media_type})", body.len());
    }
    println!(
        "  signed by {} as '{role}' -> {out}",
        personal_steward_id(sk.verifying_key().as_bytes())
    );
}

fn read_stdin() -> Vec<u8> {
    let mut buf = Vec::new();
    std::io::stdin()
        .read_to_end(&mut buf)
        .unwrap_or_else(|e| die(format!("reading stdin: {e}")));
    buf
}

// ---- status / next (the rite engine) -------------------------------------

/// Resolve the `.comms/` directory from an optional path: the path itself if it
/// holds a comms.toml, else `<path>/.comms`.
fn resolve_comms_dir(arg: Option<&str>) -> std::path::PathBuf {
    let base = std::path::Path::new(arg.unwrap_or("."));
    if base.join("comms.toml").is_file() {
        base.to_path_buf()
    } else {
        base.join(".comms")
    }
}

/// The flags `next` would need to perform `step`, for display in `status`.
fn step_hint(step: &comms_core::config::Step) -> &'static str {
    if step.verb == "attest" {
        " --body <file> [--about S] [--kind K]"
    } else if step.verb == "request" {
        " --body <file>"
    } else if step.verb == "grant" {
        " --key <counterparty key> [--decision grant|decline|defer] [--body <file>]"
    } else {
        ""
    }
}

fn cmd_status(args: &[String]) {
    let o = parse_opts(args);
    let comms_dir = resolve_comms_dir(o.positionals.first().map(String::as_str));
    let cfg = config::load(&comms_dir).unwrap_or_else(|e| die(e));
    let active = rites::active_rite(&comms_dir, &cfg);

    if o.has("--json") {
        print!("{}", status_json(&comms_dir, &cfg, active));
        return;
    }

    let archive = cfg
        .archive_mode
        .as_deref()
        .map(|m| format!(", archive {m}"))
        .unwrap_or_default();
    println!("comms: profile '{}'{archive}", cfg.profile);
    if cfg.rites.is_empty() {
        println!("  (no rites declared in comms.toml)");
    }
    for r in &cfg.rites {
        let v = rites::rite_view(&comms_dir, r);
        let here = active.map(|a| a.name == r.name).unwrap_or(false);
        println!(
            "\n  rite {}{}  [{}]",
            r.name,
            if here { " *" } else { "" },
            if v.complete() { "complete" } else { "in progress" }
        );
        for (i, sv) in v.steps.iter().enumerate() {
            let glyph = if sv.done {
                "✓"
            } else if Some(i) == v.next {
                "→"
            } else {
                "·"
            };
            println!("    {glyph} {}", sv.step.display());
        }
    }

    // A session key on disk is a liability the door should not be quiet
    // about: if its session ended without the close rite, it must be shredded
    // before a new session opens (a persisted key reads as a live session and
    // could sign as the last one).
    let key_file = comms_dir.join("session.key");
    if let Ok(meta) = std::fs::metadata(&key_file) {
        let since = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| comms_core::rfc3339_from_unix(d.as_secs()))
            .unwrap_or_else(|| "unknown time".to_owned());
        println!("\n  session key on disk since {since} — shredded at close; if that");
        println!("  session is not yours, shred before opening a new one.");
    }

    // Staged items awaiting a counterparty are a rite position too — show
    // whose signature the door is waiting on.
    let pending = comms_dir.join("pending");
    if let Ok(items) = comms_core::signing::read_pending(&pending) {
        let waiting: Vec<_> = items.iter().filter(|i| !i.needs.is_empty()).collect();
        if !waiting.is_empty() {
            println!("\n  awaiting signature in {}:", pending.display());
            for item in waiting {
                for need in &item.needs {
                    println!("    {} needs {} as {}", item.stem, need.by, need.role);
                }
            }
            println!("    they run: comms sign --key <their key> --pending {}, then comms finalize",
                pending.display());
        }
    }

    match active.map(|r| (r, rites::rite_view(&comms_dir, r))) {
        Some((r, v)) => {
            if let Some(i) = v.next {
                let step = &r.steps[i];
                println!("\nnext: {} → {}", r.name, step.display());
                println!("  run: comms next --rite {}{}", r.name, step_hint(step));
            }
        }
        None => println!("\nall declared rites complete."),
    }
}

fn status_json(
    comms_dir: &std::path::Path,
    cfg: &HarnessConfig,
    active: Option<&comms_core::config::Rite>,
) -> String {
    let rites_json: Vec<_> = cfg
        .rites
        .iter()
        .map(|r| {
            let v = rites::rite_view(comms_dir, r);
            serde_json::json!({
                "name": r.name,
                "complete": v.complete(),
                "steps": v.steps.iter().enumerate().map(|(i, s)| serde_json::json!({
                    "step": s.step.display(),
                    "verb": s.step.verb,
                    "done": s.done,
                    "next": Some(i) == v.next,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();

    let next = active.and_then(|r| {
        let v = rites::rite_view(comms_dir, r);
        v.next.map(|i| {
            let step = &r.steps[i];
            serde_json::json!({
                "rite": r.name,
                "step": step.display(),
                "command": format!("comms next --rite {}{}", r.name, step_hint(step)),
            })
        })
    });

    let out = serde_json::json!({
        "profile": cfg.profile,
        "archive_mode": cfg.archive_mode,
        "active_rite": active.map(|r| r.name.clone()),
        "rites": rites_json,
        "next": next,
    });
    format!("{}\n", serde_json::to_string_pretty(&out).unwrap())
}

fn cmd_next(args: &[String]) {
    let o = parse_opts(args);
    let comms_dir = resolve_comms_dir(o.positionals.first().map(String::as_str));
    let cfg = config::load(&comms_dir).unwrap_or_else(|e| die(e));

    let rite = match o.get("--rite") {
        Some(name) => cfg
            .rite(name)
            .unwrap_or_else(|| die(format!("no rite '{name}' declared in comms.toml"))),
        None => rites::active_rite(&comms_dir, &cfg)
            .unwrap_or_else(|| die("no rite in progress; nothing to do")),
    };

    let view = rites::rite_view(&comms_dir, rite);
    let Some(i) = view.next else {
        die(format!("rite '{}' is already complete", rite.name));
    };
    let step = &rite.steps[i];

    let body = o.get("--body").map(|p| if p == "-" { read_stdin() } else { read_file(p) });
    let inputs = ExecInputs {
        body,
        about: o.get("--about"),
        kind: o.get("--kind"),
        media_type: o.get("--media-type"),
        label: o.get("--label").unwrap_or(""),
        key: o.get("--key").map(std::path::PathBuf::from),
        decision: o.get("--decision"),
        deliver: o.get("--deliver"),
    };

    match rites::execute_step(&comms_dir, rite, step, &inputs) {
        Ok(outcome) => {
            println!("[{}] {} — {}", rite.name, step.display(), outcome.message);
            if let Some(seed) = &outcome.secret {
                println!("\n  session seed (shown once, never written to disk):");
                println!("    {seed}");
                println!("  Hold it in memory only. Later steps read it from {}=<seed>;", rites::SEED_ENV);
                println!("  at close, unset it and forget it — that act is the shred.");
            }
            match rites::rite_view(&comms_dir, rite).next {
                Some(j) => {
                    let nstep = &rite.steps[j];
                    println!("next: {}  (comms next --rite {}{})", nstep.display(), rite.name, step_hint(nstep));
                }
                None => println!("rite '{}' complete.", rite.name),
            }
        }
        Err(e) => die(e),
    }
}

fn cmd_waive(args: &[String]) {
    let o = parse_opts(args);
    let type_name = o
        .positionals
        .first()
        .map(String::as_str)
        .unwrap_or_else(|| die("usage: comms waive <type> [dir] --body <reason file|->"));
    let comms_dir = resolve_comms_dir(o.positionals.get(1).map(String::as_str));
    let cfg = config::load(&comms_dir).unwrap_or_else(|e| die(e));
    let body = o
        .get("--body")
        .map(|p| if p == "-" { read_stdin() } else { read_file(p) })
        .unwrap_or_else(|| die("a waiver needs its reason in writing: pass --body <file|->"));

    match rites::record_waiver(&comms_dir, &cfg, type_name, &body) {
        Ok(outcome) => println!("{}", outcome.message),
        Err(e) => die(e),
    }
}

// ---- verify ----------------------------------------------------------------

fn cmd_verify(args: &[String]) {
    let o = parse_opts(args);
    let path = o
        .positionals
        .first()
        .map(String::as_str)
        .unwrap_or_else(|| die("usage: comms verify <bundle.cbor>"));
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

    // Body-status summary (A2.2): informational here — `verify` judges the
    // seal; `inspect` gives the per-member picture.
    let statuses: Vec<BodyStatus> = bundle
        .attestations
        .iter()
        .filter_map(|a| match content_report(a, &bundle.media) {
            ContentReport::Detached { status, .. } => Some(status),
            _ => None,
        })
        .collect();
    if !statuses.is_empty() {
        let count = |s: BodyStatus| statuses.iter().filter(|x| **x == s).count();
        println!(
            "detached bodies: {} verified, {} absent, {} mismatched (see `inspect` for detail)",
            count(BodyStatus::Verified),
            count(BodyStatus::Absent),
            count(BodyStatus::Mismatched),
        );
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
        .unwrap_or_else(|| die("usage: comms inspect <bundle.cbor> [--json]"));
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
/// and any seal present is valid. Body status stays a separate judgment
/// (A2.2): `absent` is normal and does not fail inspection; a `mismatched`
/// body or malformed content map does — loudly, without being confused for a
/// signature failure.
fn inspect_ok(r: &InspectReport) -> bool {
    r.members.iter().all(|m| m.all_signatures_ok)
        && r.members.iter().all(|m| {
            !matches!(
                m.content,
                ContentReport::Malformed(_)
                    | ContentReport::Detached { status: BodyStatus::Mismatched, .. }
            )
        })
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
        match &m.content {
            ContentReport::None | ContentReport::Embedded { .. } => {}
            ContentReport::Detached { body_b3, body_len, status } => {
                let glyph = match status {
                    BodyStatus::Verified => "✓",
                    BodyStatus::Absent => "·",
                    BodyStatus::Mismatched => "✗",
                };
                println!(
                    "    {glyph} body [{}] detached, {} byte{} committed, blake3 {}",
                    status.as_str(),
                    body_len,
                    plural(*body_len as usize),
                    hex_str(body_b3),
                );
                if *status == BodyStatus::Mismatched {
                    println!(
                        "      bytes at hand do not match the commitment — retained and \
                         exportable, judge their provenance (A2.2)"
                    );
                }
            }
            ContentReport::Malformed(why) => {
                println!("    ✗ content MALFORMED: {why}");
            }
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
            let content = match &m.content {
                ContentReport::None => serde_json::json!(null),
                ContentReport::Embedded { len } => {
                    serde_json::json!({"form": "embedded", "body_len": len, "body_status": "verified"})
                }
                ContentReport::Detached { body_b3, body_len, status } => serde_json::json!({
                    "form": "detached",
                    "body_b3_hex": hex_str(body_b3),
                    "body_len": body_len,
                    "body_status": status.as_str(),
                }),
                ContentReport::Malformed(why) => {
                    serde_json::json!({"form": "malformed", "detail": why})
                }
            };
            serde_json::json!({
                "id": m.id,
                "claim_type": m.claim_type,
                "is_seal": m.is_seal,
                "content": content,
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
        .unwrap_or_else(|| die("usage: comms seal <bundle.cbor> --key <key.json>"));
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

    let mut members = Vec::new();
    for p in &o.positionals {
        let files = expand_cbor_paths(p);
        if files.is_empty() {
            // A directory of non-.cbor content (the common surprise) is not an
            // error, but packing it silently as zero members hides the mistake.
            eprintln!("warning: {p} contributed no .cbor attestations (skipped)");
        }
        for file in files {
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

    // A media-only bundle is legitimate; an entirely empty one is the mistake.
    // (`attest` is the path to author the .cbor members a bundle carries.)
    if members.is_empty() && media.is_empty() {
        die("pack produced an empty bundle: give at least one .cbor attestation \
             (see `attest`) or a --media file");
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
        .unwrap_or_else(|| die("usage: comms extract <bundle.cbor> --out <dir>"));
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

// ---- intake / audit (the archive profile's custody verbs) -------------------

/// The archive root: the directory whose `.comms/comms.toml` declares
/// profile "archive". Defaults to the current directory.
fn resolve_archive_root(arg: Option<&str>) -> std::path::PathBuf {
    let root = std::path::PathBuf::from(arg.unwrap_or("."));
    let comms_dir = root.join(".comms");
    match config::load(&comms_dir) {
        Ok(cfg) if cfg.profile == "archive" => root,
        Ok(cfg) => die(format!(
            "{} declares profile '{}', not 'archive' — intake/audit run at the \
             archive root (comms init <dir> --profile archive)",
            comms_dir.join("comms.toml").display(),
            cfg.profile
        )),
        Err(e) => die(format!("not an archive root: {e}")),
    }
}

fn cmd_intake(args: &[String]) {
    let o = parse_opts(args);
    let source = o
        .positionals
        .first()
        .map(String::as_str)
        .unwrap_or_else(|| {
            die("usage: comms intake <bundle|file|dir> [archive-root] --key <custodian key> \
                 [--legacy --provenance S]")
        });
    let root = resolve_archive_root(o.positionals.get(1).map(String::as_str));
    let archive = comms_core::archive::Archive::at(&root);
    let key = comms_core::signing::load_signing_key(std::path::Path::new(o.require("--key")))
        .unwrap_or_else(|e| die(e));

    let report = if o.has("--legacy") {
        let provenance = o.get("--provenance").unwrap_or_else(|| {
            die("--legacy intake records testimony: state its provenance as honestly \
                 as you can (--provenance \"found in contarchive/memory, believed session 3\")")
        });
        comms_core::archive::intake_legacy(&archive, std::path::Path::new(source), &key, provenance)
    } else {
        comms_core::archive::intake_bundle(&archive, std::path::Path::new(source), &key)
    }
    .unwrap_or_else(|e| die(format!("intake refused: {e}")));

    if report.is_noop() {
        println!(
            "intake: no-op — all {} member{} and {} bod{} already in custody; no \
             custody attestation written",
            report.kept_members,
            plural(report.kept_members),
            report.kept_bodies,
            if report.kept_bodies == 1 { "y" } else { "ies" },
        );
        return;
    }
    println!(
        "intake: {} new member{} ({} kept), {} new bod{} ({} kept)",
        report.new_members.len(),
        plural(report.new_members.len()),
        report.kept_members,
        report.new_bodies.len(),
        if report.new_bodies.len() == 1 { "y" } else { "ies" },
        report.kept_bodies,
    );
    for tag in &report.views_regenerated {
        println!("  view regenerated: views/sessions/{tag}/");
    }
    if let Some(id) = &report.custody_attestation {
        println!("  custody attested: {id}");
    }
}

fn cmd_audit(args: &[String]) {
    let o = parse_opts(args);
    let root = resolve_archive_root(o.positionals.first().map(String::as_str));
    let archive = comms_core::archive::Archive::at(&root);
    let r = comms_core::archive::audit(&archive).unwrap_or_else(|e| die(e));

    println!(
        "store:  {} intact, {} drifted",
        r.store_intact,
        r.store_drift.len()
    );
    for (name, why) in &r.store_drift {
        println!("  ✗ {name}: {why}");
    }
    println!(
        "bodies: {} intact, {} drifted, {} unreferenced (testimony or awaiting attestations)",
        r.bodies_intact,
        r.bodies_drift.len(),
        r.bodies_unreferenced
    );
    for (name, why) in &r.bodies_drift {
        println!("  ✗ {name}: {why}");
    }
    if !r.bodies_absent.is_empty() {
        println!("absent bodies (committed in store, not in custody — normal for host-gated material):");
        for (id, h) in &r.bodies_absent {
            println!("  · {id} -> blake3 {h}");
        }
    }
    if r.drift() {
        println!(
            "\ndrift found. Per the preservation stance nothing was deleted or repaired; \
             judge the bytes and consider recording the drift as a custody attestation."
        );
        process::exit(1);
    }
    println!("\naudit clean: every id and hash re-derives.");
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

// ---- sign / finalize (the counterparty's half of a rite) --------------------

/// Default pending directory: whichever known location holds staged items.
/// Ambiguity (both do) demands an explicit --pending rather than a guess.
fn resolve_pending_dir(explicit: Option<&str>) -> std::path::PathBuf {
    if let Some(p) = explicit {
        return std::path::PathBuf::from(p);
    }
    let candidates = [".comms/pending", "continuity/pending"];
    let with_items: Vec<&str> = candidates
        .iter()
        .copied()
        .filter(|d| {
            std::fs::read_dir(d)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .any(|e| e.file_name().to_string_lossy().ends_with(".needs.json"))
                })
                .unwrap_or(false)
        })
        .collect();
    match with_items.as_slice() {
        [one] => std::path::PathBuf::from(one),
        [] => die("no staged pending items in .comms/pending or continuity/pending (pass --pending DIR)"),
        _ => die("staged items in both .comms/pending and continuity/pending — pass --pending DIR"),
    }
}

fn cmd_sign(args: &[String]) {
    let o = parse_opts(args);
    let key = o.require("--key");
    let pending = resolve_pending_dir(o.get("--pending"));
    let sk = comms_core::signing::load_signing_key(std::path::Path::new(key))
        .unwrap_or_else(|e| die(e));
    let signer = personal_steward_id(sk.verifying_key().as_bytes());
    println!("signing as {signer}");

    let outcomes = comms_core::signing::sign_pending(&pending, &sk).unwrap_or_else(|e| die(e));
    if outcomes.is_empty() {
        println!("nothing pending in {}", pending.display());
        return;
    }
    let mut outstanding = 0;
    for oc in &outcomes {
        for role in &oc.signed_roles {
            println!("  {}: signed as {role}", oc.stem);
        }
        for need in &oc.skipped {
            println!("  {}: still needs {} as {} (not this key)", oc.stem, need.by, need.role);
            outstanding += 1;
        }
        if oc.signed_roles.is_empty() && oc.skipped.is_empty() {
            println!("  {}: fully signed already", oc.stem);
        }
    }
    if outstanding == 0 {
        println!("\nnext — seal what is signed:  comms finalize --pending {}", pending.display());
    }
}

fn cmd_finalize(args: &[String]) {
    let o = parse_opts(args);
    let pending = resolve_pending_dir(o.get("--pending"));
    let store = o
        .get("--store")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| pending.parent().unwrap_or(std::path::Path::new(".")).join("store"));

    let outcomes =
        comms_core::signing::finalize_pending(&pending, &store).unwrap_or_else(|e| die(e));
    if outcomes.is_empty() {
        println!("nothing pending in {}", pending.display());
        return;
    }
    for oc in &outcomes {
        match oc {
            comms_core::signing::FinalizeOutcome::Stored { stem, id, merged: 0 } => {
                println!("  {stem}: stored {id}")
            }
            comms_core::signing::FinalizeOutcome::Stored { stem, id, merged } => {
                println!("  {stem}: merged {merged} new signature(s) into {id}")
            }
            comms_core::signing::FinalizeOutcome::AlreadySealed { stem, id } => {
                println!("  {stem}: already sealed ({id}); nothing new")
            }
        }
    }
    println!("\nsealed into {} — commit it; uncommitted is invisible to the next session.", store.display());
}

// ---- vouch -----------------------------------------------------------------

fn cmd_vouch(args: &[String]) {
    let o = parse_opts(args);
    let path = o
        .positionals
        .first()
        .map(String::as_str)
        .unwrap_or_else(|| die("usage: comms vouch <bundle> --policy ID --subject ID --purpose S --as-of T"));
    let bundle = read_bundle(path);
    let store: HashMap<String, Attestation> = bundle
        .members()
        .into_iter()
        .map(|a| (a.id(), a))
        .collect();
    let query = Query {
        subject: o.require("--subject").to_owned(),
        purpose: o.require("--purpose").to_owned(),
        community: o.get("--community").map(str::to_owned),
        as_of: o.require("--as-of").to_owned(),
    };
    let result = evaluate(&store, o.require("--policy"), query)
        .unwrap_or_else(|e| die(e.to_string()));
    if o.has("--json") {
        print!("{}", vouch_json(&result));
    } else {
        print_vouch(&result);
    }
    if let Some(out) = o.get("--receipt-out") {
        let sk = load_key(o.require("--key"));
        let receipt = judgment_receipt(&result, &sk, &result.query.as_of);
        write_out(out, &receipt.to_cbor());
        eprintln!("receipt: {} -> {out}", receipt.id());
    }
}

fn print_vouch(r: &Evaluation) {
    println!("outcome: {}", r.outcome.as_str());
    println!("subject: {}", r.query.subject);
    println!("purpose: {}", r.query.purpose);
    println!("policy: {}", r.policy_id);
    println!("store view: {}", r.store_view);
    println!("positive issuers: {}", r.positive_issuers.len());
    println!("endorsers: {}", r.endorsers.len());
    println!("negative issuers: {}", r.negative_issuers.len());
    if !r.unresolved.is_empty() {
        println!("unresolved:");
        for id in &r.unresolved {
            println!("  {id}");
        }
    }
    println!("evidence:");
    for e in &r.evidence {
        println!(
            "  {} {} {} [{}] — {}",
            if e.counted { "✓" } else { "·" },
            e.class,
            e.id,
            e.issuer.as_deref().unwrap_or("no issuer"),
            e.reason
        );
    }
}

fn vouch_json(r: &Evaluation) -> String {
    let out = serde_json::json!({
        "engine": ENGINE,
        "query": {
            "subject": r.query.subject,
            "purpose": r.query.purpose,
            "community": r.query.community,
            "as_of": r.query.as_of,
        },
        "policy": r.policy_id,
        "store_view": r.store_view,
        "outcome": r.outcome.as_str(),
        "positive_issuers": r.positive_issuers,
        "negative_issuers": r.negative_issuers,
        "endorsers": r.endorsers,
        "unresolved": r.unresolved,
        "paths": r.paths,
        "evidence": r.evidence.iter().map(|e| serde_json::json!({
            "id": e.id,
            "claim_type": e.claim_type,
            "issuer": e.issuer,
            "class": e.class,
            "counted": e.counted,
            "reason": e.reason,
        })).collect::<Vec<_>>(),
    });
    format!("{}\n", serde_json::to_string_pretty(&out).unwrap())
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
