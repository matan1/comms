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
        Some("deliver") => cmd_deliver(&argv[1..]),
        Some("intake") => cmd_intake(&argv[1..]),
        Some("audit") => cmd_audit(&argv[1..]),
        Some("catalog") => cmd_catalog(&argv[1..]),
        Some("manifest") => cmd_manifest(&argv[1..]),
        Some("trial-log") => cmd_trial_log(&argv[1..]),
        Some("pending") => cmd_pending(&argv[1..]),
        Some("agent") => cmd_agent(&argv[1..]),
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
         \x20 deliver <attestation-id|body-b3> [dir] [--request ID]\n\
         \x20         host/archive side: copy a granted body to the request delivery path\n\
         \x20 intake  <bundle|dir> [root] --key <k> [--legacy --provenance S]\n\
         \x20         host/archive side: verify, ingest by id/hash, regenerate\n\
         \x20         undurable views, attest custody (idempotent)\n\
         \x20 audit   [root]                   re-derive every id and hash in custody;\n\
         \x20         mark drift, never delete; suggest custody testimony\n\
         \x20 catalog <path> [--json]          inventory an archive without changing it\n\
         \x20 manifest <path> [--level minimal|full] [--out P]\n\
         \x20         deterministic archive disclosure; never injected context\n\
         \x20 trial-log [dir] [--session N] [--out P] [--force]\n\
         \x20         render a Continuity Trial log stub from signed store evidence\n\
         \x20 pending list|inspect|state|clarify ...  appraise proposed acts\n\
         \x20 agent   serve|list [--socket PATH]  built-in key agent (ssh-agent\n\
         \x20         protocol): session seeds live in its memory, never on disk\n\
         \x20 mint    --out <key.json> [--label L]   generate a steward key for sealing\n\
         \x20 waive   <type> [dir] --body <reason>   record that this session cannot\n\
         \x20         produce a required artifact (the gap becomes an attestation)\n\
         \x20 sign    --key <openssh|steward-key> [--pending DIR] [--item ID]...\n\
         \x20         countersign every or only explicitly selected staged item\n\
         \x20         pending items (the counterparty's half of a rite)\n\
         \x20 finalize [--pending DIR] [--store DIR] [--item ID]...\n\
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
        "deliver" => "comms deliver <attestation-id|body-b3-hex> [repo-root] [--request ID]\n  Host/archive-side transport after a grant: resolve a detached body from the\n  configured archive, copy it to the grants/<request-id>/ delivery path, and\n  print the exact path and hash. If --request is omitted, uses this session's\n  recorded archive request from the continuity rite.\n",
        "intake" => "comms intake <bundle|file|dir> [archive-root] --key <custodian key> \\\n    [--legacy --provenance \"...\"]\n  Host/archive-side crossing: verify a sealed session bundle, ingest members by\n  id and bodies by hash (idempotent), regenerate undurable views/ for browsing,\n  and attest custody. --legacy takes unattested material in as testimony, by\n  hash, honestly labeled.\n",
        "audit" => "comms audit [archive-root]\n  Host/archive-side custody check: walk store/ and bodies/, re-derive every id\n  and hash, and report intact / absent / mismatched. Drift is marked, never\n  deleted. On drift, audit should propose a reviewed custody attestation draft.\n",
        "catalog" => "comms catalog <path> [--json]\n  Read-only inventory of any archive-shaped or legacy directory. Hashes and\n  classifies regular files, counts text lines by top-level category, and\n  reports exact duplicate groups. Follows no symlinks and writes nothing.\n",
        "manifest" => "comms manifest <path> [--level minimal|full] [--out P]\n  Generate a deterministic archive view. Minimal discloses only aggregate\n  structure and health; full adds artifact metadata, relationships, duplicates,\n  and gaps. Generation is automatic-capable, but inspection remains chosen.\n",
        "trial-log" => "comms trial-log [repo-root] [--session N] [--out P] [--force]\n  Render a Continuity Trial log stub from a verified, session-signed opening\n  entry. Auto-fills evidence IDs and leaves History's observations as [History].\n  Output is stdout unless --out is given; existing files require --force.\n",
        "pending" => "comms pending list [DIR]... [--json]\ncomms pending inspect <stem|id> [DIR]... [--json]\ncomms pending state <stem|id> --state S [--pending DIR]\ncomms pending clarify <stem|id> --body F --key K [--pending DIR] [--store DIR]\n  Discover and appraise proposed signing acts without conflating inboxes.\n  Clarification creates a signed question and leaves the pending core unchanged.\n",
        "mint" => "comms mint --out <key.json> [--label L]\n  Generate a steward key ({seed_b58, label} JSON, mode 0600).\n",
        "agent" => "comms agent serve|list [--socket PATH]\n  serve: run the built-in key agent (ssh-agent protocol; seeds live only in\n  its memory — its death is a shred). list: show held identities.\n  Socket resolution: --socket, $COMMS_AGENT_SOCK, $SSH_AUTH_SOCK,\n  .comms/agent.sock. With session_key = \"ssh-agent\" in comms.toml, mint\n  hands the session seed to this agent and no key file ever exists.\n",
        "waive" => "comms waive <type> [dir] --body <reason file|->\n  Record a session-signed waiver: this session cannot produce a declared\n  `required_for` artifact, and says so on the record instead of being blocked.\n  Only rites with `allow_waivers = true` accept it at seal.\n",
        "sign" => "comms sign --key <path> [--pending DIR] [--item STEM|ID]...\n  Countersign staged pending items (<name>.cbor + <name>.needs.json) with an\n  OpenSSH ed25519 key or a steward key file. --item creates a bounded signing\n  plan; omitted, every item in the explicitly resolved inbox is considered.\n",
        "finalize" => "comms finalize [--pending DIR] [--store DIR] [--item STEM|ID]...\n  Verify and move every or only explicitly selected fully-signed item into the\n  store. Selected finalization is atomic and unrelated inbox items cannot block\n  or be swept into it.\n",
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
    items: Vec<String>,
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
                    "--item" => o.items.push(val),
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
    // Steward JSON or OpenSSH ed25519 — the same forms `sign`, `intake`, and
    // the rites accept; a key that can grant must also be able to seal.
    comms_core::signing::load_signing_key(std::path::Path::new(path)).unwrap_or_else(|e| die(e))
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
    // `--key session` signs as the live session however the harness holds its
    // seed (file, agent, or ephemeral) — required in ssh-agent mode, where no
    // key file exists to point at.
    let key_arg = o.require("--key");
    let sk: Box<dyn comms_core::StewardSigner> = if key_arg == "session" {
        let comms_dir = std::path::PathBuf::from(".comms");
        let cfg = comms_core::config::load(&comms_dir).unwrap_or_else(|e| die(e));
        Box::new(
            comms_core::rites::current_session_signer(&comms_dir, &cfg)
                .unwrap_or_else(|e| die(e)),
        )
    } else {
        Box::new(load_key(key_arg))
    };
    let sk = sk.as_ref();
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
    let att = comms_core::bundle::author_general_claim_with(&spec, sk, role, signed_at)
        .unwrap_or_else(|e| die(e));
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
    println!("  signed by {} as '{role}' -> {out}", sk.steward_id());
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

fn blocked_by(comms_dir: &std::path::Path, cfg: &HarnessConfig, rite: &comms_core::config::Rite) -> Vec<String> {
    rite.requires
        .iter()
        .filter(|name| {
            cfg.rite(name)
                .map(|required| !rites::rite_view(comms_dir, required).complete())
                .unwrap_or(true)
        })
        .cloned()
        .collect()
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
        let blockers = blocked_by(&comms_dir, &cfg, r);
        if !blockers.is_empty() {
            println!("    ! blocked by incomplete rite(s): {}", blockers.join(", "));
        }
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

    // Agent-held sessions have no key file; report where the seed lives so
    // the operator can see the custody state at a glance.
    if let Ok(cfg) = comms_core::config::load(&comms_dir) {
        if cfg.session_key == "ssh-agent" {
            let sock = comms_core::sshagent::socket_path(&comms_dir);
            let sid = std::fs::read_to_string(comms_dir.join("session.id"))
                .map(|s| s.trim().to_owned())
                .unwrap_or_default();
            let held = sid
                .strip_prefix("comms.steward:z")
                .and_then(|z| bs58::decode(z).into_vec().ok())
                .and_then(|v| <[u8; 32]>::try_from(v.as_slice()).ok())
                .map(|pk| comms_core::sshagent::holds(&sock, &pk))
                .unwrap_or(false);
            if held {
                println!(
                    "\n  session seed held by the agent at {} — never on disk;",
                    sock.display()
                );
                println!("  shred removes it from the agent at close.");
            } else if !sid.is_empty() {
                println!(
                    "\n  ssh-agent mode: no agent at {} holds the recorded session key —",
                    sock.display()
                );
                println!("  the session is closed, or its agent is gone (a dead agent is a shred).");
            }
        }
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
                let blockers = blocked_by(&comms_dir, &cfg, r);
                if blockers.is_empty() {
                    println!("\nnext: {} → {}", r.name, step.display());
                    println!("  run: comms next --rite {}{}", r.name, step_hint(step));
                } else {
                    println!("\nblocked: {} requires completed rite(s): {}", r.name, blockers.join(", "));
                }
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
            let blockers = blocked_by(comms_dir, cfg, r);
            serde_json::json!({
                "name": r.name,
                "complete": v.complete(),
                "requires": r.requires,
                "blocked_by": blockers,
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
            let blockers = blocked_by(comms_dir, cfg, r);
            let command = if blockers.is_empty() {
                Some(format!("comms next --rite {}{}", r.name, step_hint(step)))
            } else {
                None
            };
            serde_json::json!({
                "rite": r.name,
                "step": step.display(),
                "blocked_by": blockers,
                "command": command,
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
        let session = rites::session_id(&comms_dir, rite)
            .unwrap_or_else(|| "no session on record".to_owned());
        die(format!(
            "rite '{}' is already complete for session {session} — if that is \
             not the session you meant to act on, this checkout is stale: pull \
             the session's branch and check .comms/session.id",
            rite.name
        ));
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
            ContentReport::Detached { status, .. }
            | ContentReport::LegacyDetached { status, .. } => Some(status),
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
                    | ContentReport::LegacyDetached { status: BodyStatus::Mismatched, .. }
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
            ContentReport::LegacyDetached { body_hash, status } => {
                let glyph = match status {
                    BodyStatus::Verified => "✓",
                    BodyStatus::Absent => "·",
                    BodyStatus::Mismatched => "✗",
                };
                println!(
                    "    {glyph} body [{}] detached (legacy body_hash, pre-A2 — hash-only \
                     commitment), blake3 {}",
                    status.as_str(),
                    hex_str(body_hash),
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
                ContentReport::LegacyDetached { body_hash, status } => serde_json::json!({
                    "form": "legacy-detached",
                    "body_hash_hex": hex_str(body_hash),
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

fn cmd_deliver(args: &[String]) {
    let o = parse_opts(args);
    let target_ref = o
        .positionals
        .first()
        .map(String::as_str)
        .unwrap_or_else(|| die("usage: comms deliver <attestation-id|body-b3-hex> [repo-root] [--request ID]"));
    let comms_dir = resolve_comms_dir(o.positionals.get(1).map(String::as_str));
    let cfg = config::load(&comms_dir).unwrap_or_else(|e| die(e));
    let archive_rite = cfg
        .rite("archive")
        .unwrap_or_else(|| die("no rite 'archive' declared in comms.toml"));
    let request_id = match o.get("--request") {
        Some(id) => id.to_owned(),
        None => rites::recorded_request_id(&comms_dir, archive_rite, "archive")
            .unwrap_or_else(|e| die(e)),
    };
    let note = rites::deliver_body(&comms_dir, target_ref, &request_id)
        .unwrap_or_else(|e| die(format!("delivery refused: {e}")));
    println!("delivered for request {request_id}{}", note.trim_end());
    println!("requester should verify the received bytes against the printed blake3 before relying on them.");
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

fn cmd_catalog(args: &[String]) {
    let o = parse_opts(args);
    let root = o.positionals.first().map(String::as_str)
        .unwrap_or_else(|| die("usage: comms catalog <path> [--json]"));
    let r = comms_core::archive::catalog(std::path::Path::new(root))
        .unwrap_or_else(|e| die(format!("catalog failed: {e}")));

    if o.has("--json") {
        let categories: serde_json::Map<String, serde_json::Value> = r.categories.iter().map(|(name, s)| (
            name.clone(),
            serde_json::json!({
                "files": s.files, "bytes": s.bytes,
                "text_files": s.text_files, "text_lines": s.text_lines,
            }),
        )).collect();
        let entries: Vec<_> = r.entries.iter().map(|e| serde_json::json!({
            "path": e.path, "bytes": e.bytes, "blake3": e.blake3,
            "kind": e.kind, "lines": e.lines,
        })).collect();
        let duplicates: Vec<_> = r.duplicates.iter().map(|d| serde_json::json!({
            "blake3": d.blake3, "bytes_each": d.bytes_each, "paths": d.paths,
        })).collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "root": r.root, "files": r.files, "bytes": r.bytes,
            "text_files": r.text_files, "text_lines": r.text_lines,
            "symlinks_skipped": r.symlinks_skipped,
            "categories": categories, "kinds": r.kinds,
            "duplicate_bytes": r.duplicate_bytes, "duplicates": duplicates,
            "entries": entries,
        })).unwrap_or_else(|e| die(format!("cannot render catalog JSON: {e}"))));
        return;
    }

    println!("catalog: {}", r.root);
    println!("{} files, {} bytes; {} text files, {} lines; {} symlink{} skipped",
        r.files, r.bytes, r.text_files, r.text_lines, r.symlinks_skipped, plural(r.symlinks_skipped));
    println!("categories:");
    for (name, s) in &r.categories {
        println!("  {name}: {} files, {} bytes, {} text lines", s.files, s.bytes, s.text_lines);
    }
    println!("kinds:");
    for (kind, count) in &r.kinds { println!("  {kind}: {count}"); }
    println!("duplicate groups: {} ({} redundant bytes)", r.duplicates.len(), r.duplicate_bytes);
    for d in &r.duplicates {
        println!("  {} — {} bytes each", d.blake3, d.bytes_each);
        for path in &d.paths { println!("    {path}"); }
    }
}

fn cmd_manifest(args: &[String]) {
    let o = parse_opts(args);
    let root = o.positionals.first().map(String::as_str)
        .unwrap_or_else(|| die("usage: comms manifest <path> [--level minimal|full] [--out P]"));
    let level = comms_core::archive::ManifestLevel::parse(o.get("--level").unwrap_or("minimal"))
        .unwrap_or_else(|e| die(e));
    let manifest = comms_core::archive::manifest(std::path::Path::new(root), level)
        .unwrap_or_else(|e| die(format!("manifest failed: {e}")));
    let rendered = serde_json::to_string_pretty(&manifest)
        .unwrap_or_else(|e| die(format!("cannot render manifest: {e}")));
    if let Some(out) = o.get("--out") {
        write_out(out, format!("{rendered}\n").as_bytes());
        println!("wrote {} manifest -> {out}", o.get("--level").unwrap_or("minimal"));
    } else {
        println!("{rendered}");
    }
}

fn cmd_trial_log(args: &[String]) {
    let o = parse_opts(args);
    let comms_dir = resolve_comms_dir(o.positionals.first().map(String::as_str));
    let session = o.get("--session").map(|value| {
        value
            .parse::<u64>()
            .unwrap_or_else(|_| die(format!("--session must be a non-negative integer, got '{value}'")))
    });
    let rendered = comms_core::trial_log::render(&comms_dir, session).unwrap_or_else(|e| die(e));
    let Some(out) = o.get("--out") else {
        print!("{rendered}");
        return;
    };
    let path = std::path::Path::new(out);
    if path.exists() && !o.has("--force") {
        die(format!("{} already exists; pass --force to replace it", path.display()));
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|e| die(format!("cannot create {}: {e}", parent.display())));
    }
    std::fs::write(path, rendered.as_bytes())
        .unwrap_or_else(|e| die(format!("cannot write {}: {e}", path.display())));
    println!("trial-log stub -> {}", path.display());
    println!("review it, then attest it before close: comms next --rite close --body {}", path.display());
}

// ---- mint ------------------------------------------------------------------

fn cmd_agent(args: &[String]) {
    let o = parse_opts(args);
    let comms_dir = std::path::PathBuf::from(".comms");
    let sock = o
        .get("--socket")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| comms_core::sshagent::socket_path(&comms_dir));
    match o.positionals.first().map(String::as_str) {
        Some("serve") => {
            // Foreground by design: the harness backgrounds it, and the
            // process's death is the shred of last resort for every seed it
            // holds. Works anywhere; a real ssh-agent (SSH_AUTH_SOCK) serves
            // the same role when present.
            println!("comms agent listening at {} (seeds live only in this process)", sock.display());
            comms_core::sshagent::serve(&sock).unwrap_or_else(|e| die(e));
        }
        Some("list") => match comms_core::sshagent::list(&sock) {
            Ok(ids) if ids.is_empty() => println!("agent at {} holds no keys", sock.display()),
            Ok(ids) => {
                for (public, comment) in ids {
                    println!("{}  {comment}", personal_steward_id(&public));
                }
            }
            Err(e) => die(e),
        },
        _ => die("usage: comms agent serve|list [--socket PATH]"),
    }
}

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

// ---- pending appraisal -----------------------------------------------------

fn default_pending_dirs() -> Vec<std::path::PathBuf> {
    vec![std::path::PathBuf::from(".comms/pending")]
}

fn pending_dirs(positionals: &[String]) -> Vec<std::path::PathBuf> {
    if positionals.is_empty() {
        default_pending_dirs()
    } else {
        positionals.iter().map(std::path::PathBuf::from).collect()
    }
}

fn pending_json(v: &comms_core::signing::PendingView, include_body: bool) -> serde_json::Value {
    let mut value = serde_json::json!({
        "source": v.source,
        "intended_store": v.intended_store,
        "stem": v.stem,
        "id": v.id,
        "claim_type": v.claim_type,
        "kind": v.kind,
        "about": v.about,
        "body_len": v.body_len,
        "signatures": v.signatures,
        "signatures_valid": v.signatures_valid,
        "needs": v.needs.iter().map(|n| serde_json::json!({
            "by": n.by, "role": n.role
        })).collect::<Vec<_>>(),
        "appraisal": v.appraisal.name(),
        "clarification_id": v.clarification_id,
    });
    if include_body {
        value["body_text"] = serde_json::json!(v.body_text);
    }
    value
}

fn select_pending<'a>(
    views: &'a [comms_core::signing::PendingView],
    selector: &str,
) -> &'a comms_core::signing::PendingView {
    let matches: Vec<_> = views
        .iter()
        .filter(|v| v.stem == selector || v.id == selector)
        .collect();
    match matches.as_slice() {
        [one] => one,
        [] => die(format!("no pending item matches '{selector}'")),
        _ => die(format!("pending selector '{selector}' matches multiple inboxes; pass --pending DIR")),
    }
}

fn cmd_pending(args: &[String]) {
    let action = args.first().map(String::as_str).unwrap_or("list");
    let o = parse_opts(if args.is_empty() { args } else { &args[1..] });
    match action {
        "list" => {
            let dirs = pending_dirs(&o.positionals);
            let views = comms_core::signing::discover_pending(&dirs).unwrap_or_else(|e| die(e));
            if o.has("--json") {
                let values: Vec<_> = views.iter().map(|v| pending_json(v, false)).collect();
                println!("{}", serde_json::to_string_pretty(&values).unwrap());
                return;
            }
            if views.is_empty() {
                println!("no pending items in {}", dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", "));
                return;
            }
            for v in &views {
                println!("{}  {}", v.stem, v.id);
                println!("  inbox: {} -> {}", v.source.display(), v.intended_store.display());
                println!("  appraisal: {}  existing signatures: {} ({})",
                    v.appraisal.name(), v.signatures,
                    if v.signatures_valid { "valid" } else { "INVALID" });
                for need in &v.needs {
                    println!("  needs: {} as {}", need.by, need.role);
                }
                if let Some(id) = &v.clarification_id {
                    println!("  clarification: {id}");
                }
            }
        }
        "inspect" => {
            let selector = o.positionals.first().map(String::as_str)
                .unwrap_or_else(|| die("usage: comms pending inspect <stem|id> [DIR]... [--json]"));
            let dirs = pending_dirs(&o.positionals[1..]);
            let views = comms_core::signing::discover_pending(&dirs).unwrap_or_else(|e| die(e));
            let view = select_pending(&views, selector);
            if o.has("--json") {
                println!("{}", serde_json::to_string_pretty(&pending_json(view, true)).unwrap());
            } else {
                println!("{}  {}", view.stem, view.id);
                println!("inbox: {}", view.source.display());
                println!("destination: {}", view.intended_store.display());
                println!("claim: {} / {} about {}",
                    view.claim_type.as_deref().unwrap_or("unknown"),
                    view.kind.as_deref().unwrap_or("unknown"),
                    view.about.as_deref().unwrap_or("unknown"));
                println!("appraisal: {}", view.appraisal.name());
                println!("existing signatures: {} ({})", view.signatures,
                    if view.signatures_valid { "valid" } else { "INVALID" });
                for need in &view.needs { println!("needs {} as {}", need.by, need.role); }
                println!("body ({} bytes):", view.body_len.unwrap_or(0));
                println!("{}", view.body_text.as_deref().unwrap_or("[detached or binary]"));
            }
        }
        "state" => {
            let selector = o.positionals.first().map(String::as_str)
                .unwrap_or_else(|| die("usage: comms pending state <stem|id> --state S [--pending DIR]"));
            let dirs = o.get("--pending")
                .map(|d| vec![std::path::PathBuf::from(d)])
                .unwrap_or_else(default_pending_dirs);
            let views = comms_core::signing::discover_pending(&dirs).unwrap_or_else(|e| die(e));
            let view = select_pending(&views, selector);
            let state = comms_core::signing::AppraisalState::parse(o.require("--state"))
                .unwrap_or_else(|e| die(e));
            let path = comms_core::signing::set_appraisal_state(&view.source, &view.stem, state, None)
                .unwrap_or_else(|e| die(e));
            println!("updated appraisal -> {}", path.display());
        }
        "clarify" => {
            let selector = o.positionals.first().map(String::as_str)
                .unwrap_or_else(|| die("usage: comms pending clarify <stem|id> --body F --key K [--pending DIR] [--store DIR]"));
            let dirs = o.get("--pending")
                .map(|d| vec![std::path::PathBuf::from(d)])
                .unwrap_or_else(default_pending_dirs);
            let views = comms_core::signing::discover_pending(&dirs).unwrap_or_else(|e| die(e));
            let view = select_pending(&views, selector);
            let body_path = o.require("--body");
            let body = if body_path == "-" { read_stdin() } else { read_file(body_path) };
            let key = comms_core::signing::load_signing_key(std::path::Path::new(o.require("--key")))
                .unwrap_or_else(|e| die(e));
            let store = o.get("--store").map(std::path::PathBuf::from)
                .unwrap_or_else(|| view.intended_store.clone());
            let out = comms_core::signing::request_clarification(
                &view.source, &view.stem, &body, &key, &store, &now_rfc3339(),
            ).unwrap_or_else(|e| die(e));
            println!("clarification requested: {}", out.clarification_id);
            println!("  pending core unchanged: {}", out.pending_id);
            println!("  attestation: {}", out.stored_at.display());
            println!("  appraisal: awaiting-clarification ({})", out.review_at.display());
        }
        _ => die(format!("unknown pending action '{action}' (use list, inspect, state, or clarify)")),
    }
}

// ---- sign / finalize (the counterparty's half of a rite) --------------------

/// Default pending directory: the repo's single staged inbox, .comms/pending.
fn resolve_pending_dir(explicit: Option<&str>) -> std::path::PathBuf {
    if let Some(p) = explicit {
        return std::path::PathBuf::from(p);
    }
    let dir = std::path::PathBuf::from(".comms/pending");
    let has_items = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().ends_with(".needs.json"))
        })
        .unwrap_or(false);
    if !has_items {
        die("no staged pending items in .comms/pending (pass --pending DIR)");
    }
    dir
}

fn cmd_sign(args: &[String]) {
    let o = parse_opts(args);
    let key = o.require("--key");
    let pending = resolve_pending_dir(o.get("--pending"));
    let sk = comms_core::signing::load_signing_key(std::path::Path::new(key))
        .unwrap_or_else(|e| die(e));
    let signer = personal_steward_id(sk.verifying_key().as_bytes());
    println!("signing as {signer}");

    let outcomes = comms_core::signing::sign_pending_selected(&pending, &sk, &o.items)
        .unwrap_or_else(|e| die(e));
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

    let outcomes = comms_core::signing::finalize_pending_selected(&pending, &store, &o.items)
        .unwrap_or_else(|e| die(e));
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
