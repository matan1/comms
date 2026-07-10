# comms-tui

Optional Ratatui appraisal workbench around `comms-core`. It is a separate
binary: the portable `comms` CLI has no terminal-UI dependency.

```sh
cargo build --release -p comms-tui
target/release/comms-tui [repo-root] --archive <archive-root> [--key <signing-key>]
```

The key is optional for browsing and local appraisal. Load one to sign a
selected pending item or author a signed clarification request. Encrypted
OpenSSH keys prompt before terminal raw mode begins.

## Keys

- `Tab`: pending desk / archive manifest
- `j`, `k`, arrows: move or scroll
- `m`: deliberately switch minimal/full manifest
- `a`, `d`, `x`, `Q`: approve, defer, decline, quarantine locally
- `c`: write and sign a clarification request about the selected core
- `s`: sign only the selected pending item
- `f`: finalize only the selected item into its displayed store
- `r`: refresh
- `q`: quit

Local appraisal state is workflow, not protocol authority. Clarification,
signing, and finalized attestations are external signed acts. The exact core
operations are also available through `comms pending`, `comms sign --item`,
and `comms finalize --item`.

`--check` loads pending state and the minimal manifest without entering a
terminal, useful for installation smoke tests.
