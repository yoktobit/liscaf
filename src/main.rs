//! Simple scaffolder: clones a repo, replaces template tokens, and initializes a new git repo.
//!
//! Usage:
//!   liscaf <new-project-name> [repo-url]
//!
//! Templates can be selected from a repositories.txt list by providing a
//! templates source (folder, repo, or http base URL).
//!
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clap::Parser;
use convert_case::{Case, Casing};
use inquire::{Confirm, Select, Text};
use walkdir::WalkDir;

/// Simple scaffolder: clones a repo, replaces template tokens, and initializes a new git repo.
#[derive(Parser, Debug)]
#[command(name = "liscaf", about = "Simple scaffolder using inquire")]
struct Args {
    /// New project name (used to replace template tokens)
    new_name: String,

    /// Git repo URL (HTTPS or SSH). Examples: https://github.com/owner/repo or git@github.com:owner/repo.git
    repo_url: Option<String>,
    /// Templates source (folder with repositories.txt, git repo, or HTTP base URL)
    #[arg(
        long = "templates",
        env = "LISCAF_TEMPLATES",
        value_name = "PATH_OR_URL",
        default_value = "github.com/yoktobit/liscaf-assets"
    )]
    templates_source: String,
    /// If set, show planned changes but don't write files or initialize git
    #[arg(long)]
    dry_run: bool,
    /// Assume yes to all prompts (non-interactive)
    #[arg(short = 'y', long = "yes")]
    yes: bool,
    /// Merge scaffold output into an existing directory instead of creating a new one
    #[arg(long = "into", value_name = "PATH")]
    into: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Ask interactively whether to keep or edit the provided values (skip if --yes)
    let assume_yes = args.yes;
    let mut new_name = args.new_name;
    if !assume_yes {
        if !Confirm::new(&format!("Use new project name '{}'?", new_name))
            .with_default(true)
            .prompt()? {
            new_name = Text::new("Enter new project name:")
                .with_placeholder("my-cool-app")
                .prompt()?;
        }
    }

    let mut repo_url = args.repo_url.unwrap_or_default();
    if repo_url.is_empty() {
        if assume_yes {
            anyhow::bail!("repo URL must be provided when running non-interactively");
        }
        repo_url = prompt_for_repo_url(&args.templates_source)?;
    } else if !assume_yes {
        if !Confirm::new(&format!("Use repo URL '{}' ?", repo_url))
            .with_default(true)
            .prompt()? {
            repo_url = prompt_for_repo_url(&args.templates_source)?;
        }
    }

    // Template base name to replace (default: acme-app)
    let mut template_base = "acme-app".to_string();
    if !assume_yes {
        if !Confirm::new(&format!("Replace occurrences of '{}' ?", template_base))
            .with_default(true)
            .prompt()? {
            template_base = Text::new("Enter template base name to replace (e.g. acme-app)")
                .with_placeholder("acme-app")
                .prompt()?;
        }
    }

    if !assume_yes {
        let proceed_msg = if let Some(ref into_dir) = args.into {
            format!(
                "Proceed to scaffold '{}'\nfrom '{}' replacing '{}'\ninto '{}' ?",
                new_name,
                repo_url,
                template_base,
                into_dir.display()
            )
        } else {
            format!(
                "Proceed to scaffold '{}'\nfrom '{}' replacing '{}' ?",
                new_name, repo_url, template_base
            )
        };

        if !Confirm::new(&proceed_msg).with_default(true).prompt()? {
            println!("Aborted by user.");
            return Ok(());
        }
    }

    let dry_run = args.dry_run;
    // Run scaffold (synchronous, prints to stdout)
    let repo_url = normalize_repo_url(&repo_url);
    run_scaffold(
        &repo_url,
        &new_name,
        &template_base,
        dry_run,
        args.into.as_deref(),
    )?;

    Ok(())
}

fn merge_into_dest(src: &Path, dest: &Path, dry_run: bool) -> anyhow::Result<()> {
    println!("Merging scaffold into {}", dest.display());
    let walker = WalkDir::new(src).into_iter();
    for entry in walker.filter_map(|e| e.ok()) {
        let src_path = entry.path();
        if src_path.components().any(|c| c.as_os_str() == ".git") {
            continue;
        }
        let rel = match src_path.strip_prefix(src) {
            Ok(r) if !r.as_os_str().is_empty() => r,
            _ => continue,
        };
        let dest_path = dest.join(rel);

        if entry.file_type().is_dir() {
            if dry_run {
                println!("DRY DIR: {}", dest_path.display());
            } else {
                fs::create_dir_all(&dest_path)?;
            }
            continue;
        }

        if !entry.file_type().is_file() {
            continue;
        }

        if !dest_path.exists() {
            if dry_run {
                println!("DRY ADD: {}", dest_path.display());
            } else {
                if let Some(parent) = dest_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(src_path, &dest_path)?;
                println!("ADD: {}", dest_path.display());
            }
            continue;
        }

        let src_bytes = fs::read(src_path)?;
        let dest_bytes = fs::read(&dest_path)?;
        if src_bytes == dest_bytes {
            continue;
        }

        let src_text = bytes_to_text(&src_bytes);
        let dest_text = bytes_to_text(&dest_bytes);

        match (src_text, dest_text) {
            (Some(incoming), Some(existing)) => {
                let merged = format!(
                    "<<<<<<< EXISTING\n{}\n=======\n{}\n>>>>>>> TEMPLATE\n",
                    existing, incoming
                );
                if dry_run {
                    println!("DRY MERGE: {}", dest_path.display());
                } else {
                    fs::write(&dest_path, merged.as_bytes())?;
                    println!("MERGE: {}", dest_path.display());
                }
            }
            _ => {
                let incoming_path = unique_suffixed_path(&dest_path, ".liscaf-incoming");
                let conflict_path = unique_suffixed_path(&dest_path, ".liscaf-conflict");
                let note = format!(
                    "<<<<<<< EXISTING\n(binary file kept at {})\n=======\n(binary incoming saved at {})\n>>>>>>> TEMPLATE\n",
                    dest_path.display(),
                    incoming_path.display()
                );
                if dry_run {
                    println!(
                        "DRY BIN CONFLICT: {} (incoming -> {})",
                        dest_path.display(),
                        incoming_path.display()
                    );
                } else {
                    if let Some(parent) = incoming_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(&incoming_path, &src_bytes)?;
                    fs::write(&conflict_path, note.as_bytes())?;
                    println!(
                        "BIN CONFLICT: {} (incoming -> {})",
                        dest_path.display(),
                        incoming_path.display()
                    );
                }
            }
        }
    }
    Ok(())
}

fn bytes_to_text(bytes: &[u8]) -> Option<String> {
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes.to_vec()).ok()
}

fn unique_suffixed_path(base: &Path, suffix: &str) -> PathBuf {
    let file_name = base
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let candidate = base.with_file_name(format!("{}{}", file_name, suffix));
    if !candidate.exists() {
        return candidate;
    }
    let mut i = 1;
    loop {
        let next = base.with_file_name(format!("{}{}{}", file_name, suffix, i));
        if !next.exists() {
            return next;
        }
        i += 1;
    }
}

fn run_scaffold(
    repo_url: &str,
    new_name: &str,
    template_base: &str,
    dry_run: bool,
    into_dir: Option<&Path>,
) -> anyhow::Result<()> {
    println!("Starting scaffolding for '{}'", new_name);
    println!("Repo URL: {}", repo_url);

    if !is_supported_repo_url(repo_url) {
        anyhow::bail!("Repo URL must be HTTPS, SSH (ssh://), or SCP-like (git@host:owner/repo.git)");
    }

    // Create a temporary directory
    let tmpdir = tempfile::Builder::new()
        .prefix("liscaf-")
        .tempdir()
        .map_err(|e| anyhow::anyhow!(e))?;
    let tmp_path = tmpdir.path().to_path_buf();
    println!("Cloning into temporary dir: {}", tmp_path.display());

    // git clone --depth 1 <url> <tmp_path>
    let clone_status = Command::new("git")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg(repo_url)
        .arg(&tmp_path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status();

    match clone_status {
        Ok(status) if status.success() => println!("git clone succeeded"),
        Ok(status) => anyhow::bail!("git clone failed with code: {}", status.code().unwrap_or(-1)),
        Err(e) => anyhow::bail!("Failed to run git: {}", e),
    }

    // Remove .git
    let git_dir = tmp_path.join(".git");
    if git_dir.exists() {
        println!("Removing .git to unlink original repository");
        if let Err(e) = fs::remove_dir_all(&git_dir) {
            println!("Warning: failed to remove .git: {}", e);
        }
    } else {
        println!("Warning: .git not found after clone");
    }

    // Build mappings
    let template_tokens = split_name_to_tokens(template_base);
    let new_tokens = split_name_to_tokens(new_name);
    println!("Template tokens: {:?}", template_tokens);
    println!("New tokens: {:?}", new_tokens);
    let mappings = generate_variant_mappings(&template_tokens, &new_tokens);
    println!("Generated {} variant mappings", mappings.len());
    for (o, n) in &mappings {
        println!("  {} -> {}", o, n);
    }

    // Replace in files
    replace_in_files(&tmp_path, &mappings, dry_run)?;

    // Rename paths
    rename_paths(&tmp_path, &mappings, dry_run)?;

    if let Some(dest_dir) = into_dir {
        if !dest_dir.exists() {
            anyhow::bail!("Destination directory does not exist: {}", dest_dir.display());
        }
        if !dest_dir.is_dir() {
            anyhow::bail!("Destination is not a directory: {}", dest_dir.display());
        }

        merge_into_dest(&tmp_path, dest_dir, dry_run)?;
        if dry_run {
            println!("Dry run: skipping merge write.");
        } else {
            println!("Merge finished");
        }
        return Ok(());
    }

    if dry_run {
        println!("Dry run: skipping git init, commit, and moving files.");
        println!("Temporary directory with changes: {}", tmp_path.display());
        println!("Scaffolding dry-run finished");
    } else {
        // Git init + commit
        println!("Initializing new git repository");
        let init_status = Command::new("git").arg("init").current_dir(&tmp_path).status();
        if let Ok(s) = init_status {
            if s.success() {
                println!("git init succeeded");
                let _ = Command::new("git").arg("add").arg(".").current_dir(&tmp_path).status();
                let _ = Command::new("git")
                    .arg("commit")
                    .arg("-m")
                    .arg("Initial commit from template (liscaf)")
                    .current_dir(&tmp_path)
                    .status();
                println!("Created initial commit");
            } else {
                println!("Warning: git init failed");
            }
        } else {
            println!("Warning: could not run git init (git not available?)");
        }

        // Move temp dir to destination
        let dest = std::env::current_dir()?.join(new_name);
        if dest.exists() {
            let dest_alt = std::env::current_dir()?.join(format!("{}_from_template", new_name));
            fs::rename(&tmp_path, &dest_alt)?;
            println!("Wrote scaffold into {}", dest_alt.display());
        } else {
            fs::rename(&tmp_path, &dest)?;
            println!("Wrote scaffold into {}", dest.display());
        }

        println!("Scaffolding finished");
    }

    Ok(())
}

fn is_supported_repo_url(repo_url: &str) -> bool {
    let lowered = repo_url.to_lowercase();
    if lowered.starts_with("https://") || lowered.starts_with("http://") {
        return true;
    }
    if lowered.starts_with("ssh://") {
        return true;
    }
    // SCP-like syntax: user@host:owner/repo(.git)
    repo_url.contains('@') && repo_url.contains(':')
}

#[derive(Debug, Clone)]
struct TemplateEntry {
    label: String,
    url: String,
}

fn prompt_for_repo_url(templates_source: &str) -> anyhow::Result<String> {
    let templates = match load_template_entries(templates_source) {
        Ok(entries) => entries,
        Err(e) => {
            println!("Warning: failed to load templates: {}", e);
            Vec::new()
        }
    };

    if templates.is_empty() {
        return Text::new("Enter repository URL (HTTPS or SSH):")
            .with_placeholder("https://github.com/owner/repo or git@github.com:owner/repo.git")
            .prompt()
            .map_err(|e| anyhow::anyhow!(e));
    }

    let manual_label = "Enter URL manually".to_string();
    let mut options: Vec<String> = templates.iter().map(|t| t.label.clone()).collect();
    options.push(manual_label.clone());

    let choice = Select::new("Choose a template:", options).prompt()?;
    if choice == manual_label {
        return Text::new("Enter repository URL (HTTPS or SSH):")
            .with_placeholder("https://github.com/owner/repo or git@github.com:owner/repo.git")
            .prompt()
            .map_err(|e| anyhow::anyhow!(e));
    }

    let selected = templates
        .into_iter()
        .find(|t| t.label == choice)
        .map(|t| t.url)
        .unwrap_or(choice);
    Ok(selected)
}

fn normalize_repo_url(repo_url: &str) -> String {
    let trimmed = repo_url.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lowered = trimmed.to_lowercase();
    if lowered.starts_with("http://")
        || lowered.starts_with("https://")
        || lowered.starts_with("ssh://")
        || (trimmed.contains('@') && trimmed.contains(':'))
    {
        return trimmed.to_string();
    }
    if trimmed.contains('/') {
        return format!("https://{}", trimmed);
    }
    trimmed.to_string()
}

fn load_template_entries(source: &str) -> anyhow::Result<Vec<TemplateEntry>> {
    let content = if source.starts_with("http://") || source.starts_with("https://") {
        load_repositories_from_http(source)?
    } else if Path::new(source).exists() {
        load_repositories_from_path(source)?
    } else {
        let repo_url = normalize_repo_url(source);
        load_repositories_from_repo(&repo_url)?
    };

    Ok(parse_template_entries(&content))
}

fn load_repositories_from_path(path: &str) -> anyhow::Result<String> {
    let repo_file = Path::new(path).join("repositories.txt");
    if !repo_file.exists() {
        anyhow::bail!("repositories.txt not found at {}", repo_file.display());
    }
    Ok(fs::read_to_string(repo_file)?)
}

fn load_repositories_from_http(base_url: &str) -> anyhow::Result<String> {
    let mut url = base_url.to_string();
    if !url.ends_with('/') {
        url.push('/');
    }
    url.push_str("repositories.txt");
    let response = ureq::get(&url)
        .call()
        .map_err(|e| anyhow::anyhow!("HTTP error fetching {}: {}", url, e))?;
    Ok(response.into_string()?)
}

fn load_repositories_from_repo(repo_url: &str) -> anyhow::Result<String> {
    if !is_supported_repo_url(repo_url) {
        anyhow::bail!("Template source repo URL is not supported: {}", repo_url);
    }

    let tmpdir = tempfile::Builder::new()
        .prefix("liscaf-templates-")
        .tempdir()
        .map_err(|e| anyhow::anyhow!(e))?;
    let tmp_path = tmpdir.path().to_path_buf();

    let clone_status = Command::new("git")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg(repo_url)
        .arg(&tmp_path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status();

    match clone_status {
        Ok(status) if status.success() => {
            let repo_file = tmp_path.join("repositories.txt");
            if !repo_file.exists() {
                anyhow::bail!("repositories.txt not found in template repo: {}", repo_url);
            }
            Ok(fs::read_to_string(repo_file)?)
        }
        Ok(status) => anyhow::bail!("git clone failed with code: {}", status.code().unwrap_or(-1)),
        Err(e) => anyhow::bail!("Failed to run git: {}", e),
    }
}

fn parse_template_entries(content: &str) -> Vec<TemplateEntry> {
    let mut entries = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (label, url) = parse_template_line(trimmed);
        let url = normalize_repo_url(&url);
        if !url.is_empty() {
            entries.push(TemplateEntry { label, url });
        }
    }
    entries
}

fn parse_template_line(line: &str) -> (String, String) {
    if let Some((left, right)) = line.split_once('|') {
        let label = left.trim();
        let url = right.trim();
        if !label.is_empty() && !url.is_empty() {
            return (label.to_string(), url.to_string());
        }
    }
    if let Some((left, right)) = line.split_once('=') {
        let label = left.trim();
        let url = right.trim();
        if !label.is_empty() && !url.is_empty() {
            return (label.to_string(), url.to_string());
        }
    }
    (line.to_string(), line.to_string())
}

/// Splits an arbitrary name like "my-cool_app" or "MyCoolApp" into tokens: ["my","cool","app"]
fn split_name_to_tokens(name: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let parts: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() <= 1 {
        let mut current = String::new();
        for ch in name.chars() {
            if ch.is_uppercase() && !current.is_empty() {
                tokens.push(current.to_lowercase());
                current = String::new();
            }
            current.push(ch);
        }
        if !current.is_empty() {
            tokens.push(current.to_lowercase());
        }
    } else {
        for p in parts {
            tokens.push(p.to_lowercase());
        }
    }
    tokens
}

fn generate_variant_mappings(orig_tokens: &[String], new_tokens: &[String]) -> Vec<(String, String)> {
    let mut pairs = Vec::<(String, String)>::new();

    let join_kebab = |t: &[String]| t.join("-");
    let join_snake = |t: &[String]| t.join("_");
    let join_upper_snake = |t: &[String]| {
        t.iter().map(|s| s.to_uppercase()).collect::<Vec<_>>().join("_")
    };
    let join_concat_lower = |t: &[String]| t.join("");
    let join_concat_upper = |t: &[String]| t.iter().map(|s| s.to_uppercase()).collect::<Vec<_>>().join("");
    let join_camel_lower = |t: &[String]| {
        if t.is_empty() { return "".to_string(); }
        let mut s = t[0].clone();
        for p in t.iter().skip(1) { s.push_str(&p.to_case(Case::Pascal)); }
        s
    };
    let join_camel_upper = |t: &[String]| {
        let mut s = String::new();
        for p in t { s.push_str(&p.to_case(Case::Pascal)); }
        s
    };
    let join_pascal_with_underscore = |t: &[String]| {
        t.iter().map(|p| p.to_case(Case::Pascal)).collect::<Vec<_>>().join("_")
    };

    let variants: Vec<(String, String)> = vec![
        (join_kebab(orig_tokens), join_kebab(new_tokens)),
        (join_snake(orig_tokens), join_snake(new_tokens)),
        (join_upper_snake(orig_tokens), join_upper_snake(new_tokens)),
        (join_concat_lower(orig_tokens), join_concat_lower(new_tokens)),
        (join_concat_upper(orig_tokens), join_concat_upper(new_tokens)),
        (join_camel_lower(orig_tokens), join_camel_lower(new_tokens)),
        (join_camel_upper(orig_tokens), join_camel_upper(new_tokens)),
        (
            join_pascal_with_underscore(orig_tokens),
            join_pascal_with_underscore(new_tokens),
        ),
    ];

    for (o, n) in variants {
        if !o.is_empty() && !n.is_empty() {
            pairs.push((o, n));
        }
    }

    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

fn replace_in_files(base: &Path, mappings: &[(String, String)], dry_run: bool) -> anyhow::Result<()> {
    println!("Replacing content inside files...");
    let walker = WalkDir::new(base).into_iter();
    for entry in walker.filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let path = entry.path();
            if path.components().any(|c| c.as_os_str() == ".git") {
                continue;
            }
            let mut buf = Vec::new();
            if let Ok(mut f) = fs::File::open(path) {
                if let Ok(_) = f.read_to_end(&mut buf) {
                    if buf.contains(&0) { continue; }
                    if let Ok(mut content) = String::from_utf8(buf) {
                        let original = content.clone();
                        for (o, n) in mappings {
                            if content.contains(o) {
                                content = content.replace(o, n);
                            }
                        }
                        if content != original {
                            if dry_run {
                                println!("DRY REPL: Would update file: {}", path.display());
                            } else {
                                if let Ok(mut f2) = fs::File::create(path) {
                                    if let Err(e) = f2.write_all(content.as_bytes()) {
                                        println!("WARN: Failed to write file {}: {}", path.display(), e);
                                    } else {
                                        println!("REPL: Updated file: {}", path.display());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn rename_paths(base: &Path, mappings: &[(String, String)], dry_run: bool) -> anyhow::Result<()> {
    println!("Renaming files and directories where needed...");
    let mut entries: Vec<PathBuf> = WalkDir::new(base)
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.into_path())
        .collect();
    entries.sort_by_key(|p| std::cmp::Reverse(p.components().count()));

    for path in entries {
        let file_name_opt = path.file_name().and_then(|s| s.to_str()).map(|s| s.to_string());
        if file_name_opt.is_none() { continue; }
        let file_name = file_name_opt.unwrap();
        let mut new_name = file_name.clone();
        for (o, n) in mappings {
            if new_name.contains(o) {
                new_name = new_name.replace(o, n);
            }
        }
        if new_name != file_name {
            let new_path = path.with_file_name(&new_name);
            let final_path = if new_path.exists() {
                let mut alt = new_path.clone();
                let mut i = 1;
                while alt.exists() {
                    alt = new_path.with_file_name(format!("{}_{}", new_name, i));
                    i += 1;
                }
                alt
            } else {
                new_path
            };
            if dry_run {
                println!("DRY RENAME: {} -> {}", path.display(), final_path.display());
            } else {
                if let Err(e) = fs::rename(&path, &final_path) {
                    println!("WARN: Failed to rename {} -> {}: {}", path.display(), final_path.display(), e);
                } else {
                    println!("RENAME: {} -> {}", path.display(), final_path.display());
                }
            }
        }
    }

    Ok(())
}
