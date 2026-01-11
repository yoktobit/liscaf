# liscaf — Lightweight scaffolder

liscaf clones a GitHub repository (HTTPS), replaces occurrences of a template name (e.g. `acme-app` in multiple case styles) with a new project name, renames files/dirs where necessary, and initializes a fresh git repository.

Requirements
- `git` in PATH
- Rust toolchain to build from source

Usage

Build:

```bash
cargo build --release
```

Run (non-interactive):

```bash
cargo run -- <new-project-name> <repo-url>
```

Examples:

```bash
cargo run -- my-cool-app https://github.com/owner/acme-app
```

Interactive prompts

If you pass values on the CLI the program will ask you to confirm and optionally edit them using interactive prompts.

Dry run

Use `--dry-run` to preview replacements and renames without modifying files or initializing git:

```bash
cargo run -- my-cool-app https://github.com/owner/acme-app --dry-run
```

Non-interactive

Use `--yes` or `-y` to skip interactive confirmations and run non-interactively (requires `repo-url` provided):

```bash
cargo run -- my-cool-app https://github.com/owner/acme-app --yes
```

Notes
- The tool removes the cloned repository's `.git` directory to unlink from the original repository before making changes, and then initializes a new repo (unless `--dry-run` is used).
- The tool performs simple textual replacements (heuristic: skips binary files).
