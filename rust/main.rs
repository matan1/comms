use std::process;

use comms_core::bundle::{parse_bundle, verify_seal};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 || args[1] == "--help" || args[1] == "-h" {
        eprintln!("usage: comms-verify <bundle.cbor>");
        eprintln!();
        eprintln!("Verify the A1.8 integrity seal of a sneakernet bundle.");
        eprintln!("Exits 0 if the seal is present and valid, 1 otherwise.");
        eprintln!();
        eprintln!("A valid seal means:");
        eprintln!("  - the seal's signature checks out (math holds)");
        eprintln!("  - the bundle hash matches the sealed manifest");
        eprintln!("  - the member set matches exactly (no drops, no additions)");
        eprintln!();
        eprintln!("'Valid seal' is layer 2/3 only (verified + resolvable).");
        eprintln!("Trust is a judgment — this tool does not make it for you.");
        process::exit(if args.len() == 1 { 1 } else { 0 });
    }

    let path = &args[1];
    let data = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: cannot read {path}: {e}");
            process::exit(1);
        }
    };

    let bundle = match parse_bundle(&data) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    let members: Vec<_> = {
        let seals: std::collections::HashSet<String> = {
            // re-identify seals by claim type to avoid duplicating find_seals logic
            bundle
                .attestations
                .iter()
                .filter(|a| {
                    a.core
                        .get("c")
                        .and_then(|c| c.get("t"))
                        .and_then(comms_core::Value::as_text)
                        == Some("general-claim/1")
                        && a.core
                            .get("c")
                            .and_then(|c| c.get("content"))
                            .and_then(|ct| ct.get("media_type"))
                            .and_then(comms_core::Value::as_text)
                            == Some("application/cbor")
                })
                .map(|a| comms_core::attestation_id(&a.core))
                .collect()
        };
        bundle
            .attestations
            .iter()
            .filter(|a| !seals.contains(&comms_core::attestation_id(&a.core)))
            .collect()
    };

    println!(
        "bundle: {} attestation{} ({} member{}, {} seal{})",
        bundle.attestations.len(),
        if bundle.attestations.len() == 1 { "" } else { "s" },
        members.len(),
        if members.len() == 1 { "" } else { "s" },
        bundle.attestations.len() - members.len(),
        if bundle.attestations.len() - members.len() == 1 { "" } else { "s" },
    );

    if !bundle.media.is_empty() {
        println!("media: {} blob{}", bundle.media.len(), if bundle.media.len() == 1 { "" } else { "s" });
    }

    let report = verify_seal(&bundle);

    if let Some(ref by) = report.sealed_by {
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
    if !report.missing.is_empty() {
        println!("  missing members ({}):", report.missing.len());
        for id in &report.missing {
            println!("    {id}");
        }
    }
    if !report.extra.is_empty() {
        println!("  extra members ({}):", report.extra.len());
        for id in &report.extra {
            println!("    {id}");
        }
    }
    process::exit(1);
}
