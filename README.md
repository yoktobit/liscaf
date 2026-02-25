# liscaf — Lightweight scaffolder

![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)

liscaf clones a GitHub repository (HTTPS), replaces occurrences of a template name (e.g. `acme-app` in multiple case styles) with a new project name, renames files/dirs where necessary, and initializes a fresh git repository.

Requirements
- `git` in PATH
- Rust toolchain to build from source

Usage

Build:

```bash
cargo build --release
```

Run scaffold (non-interactive):

```bash
cargo run -- scaffold <new-project-name> <repo-url>
```

Examples:

```bash
cargo run -- scaffold my-cool-app https://github.com/owner/acme-app
```

Merge into an existing directory (adds new files, marks conflicts with git-style markers):

```bash
cargo run -- scaffold my-cool-app https://github.com/owner/acme-app --into /path/to/existing/project
```

Interactive prompts

If you pass values on the CLI the program will ask you to confirm and optionally edit them using interactive prompts.

Template list format

When using `--templates` with a folder/repo/HTTP base URL, liscaf reads `repositories.yaml` (or `repositories.yml`).
Supported YAML formats:

```yaml
- name: "My Template"
	url: "https://github.com/owner/repo"
```

or:

```yaml
repositories:
	- name: "My Template"
		url: "https://github.com/owner/repo"
```

Dry run

Use `--dry-run` to preview replacements and renames without modifying files or initializing git:

```bash
cargo run -- scaffold my-cool-app https://github.com/owner/acme-app --dry-run
```

Non-interactive

Use `--yes` or `-y` to skip interactive confirmations and run non-interactively (requires `repo-url` provided):

```bash
cargo run -- scaffold my-cool-app https://github.com/owner/acme-app --yes
```

Replace tokens in an existing directory (content + paths):

```bash
cargo run -- replace myOtherSentence newProjectSentence
```

Optional path and dry run:

```bash
cargo run -- replace myOtherSentence newProjectSentence --path /path/to/target --dry-run
```

Notes
- The tool removes the cloned repository's `.git` directory to unlink from the original repository before making changes, and then initializes a new repo (unless `--dry-run` is used).
- The tool performs simple textual replacements (heuristic: skips binary files).
- When using `--into`, files are merged into the destination folder. If a file already exists and differs, a conflict is written using git-style markers. Binary conflicts are saved as a separate `.liscaf-incoming` file with a `.liscaf-conflict` note.

Scaffold metadata

Each scaffolded project includes a root `.scaffold.json` file with generation metadata:

```json
{
	"project_name": "my-cool-app",
	"template_repo_url": "https://github.com/owner/acme-app",
	"template_base": "acme-app",
	"generator": "liscaf",
	"generated_at": "2026-02-24T12:34:56Z"
}
```

`generated_at` is an ISO-8601 UTC timestamp.

License

MIT. See [LICENSE](LICENSE).
