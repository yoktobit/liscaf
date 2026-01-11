//! Simple scaffolder: clones a repo, replaces template tokens, and initializes a new git repo.
//!
//! Usage:
//!   liscaf <new-project-name> <repo-url>
//!
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clap::Parser;
use convert_case::{Case, Casing};
use inquire::{Confirm, Text};
use walkdir::WalkDir;

/// Simple scaffolder: clones a repo, replaces template tokens, and initializes a new git repo.
#[derive(Parser, Debug)]
#[command(name = "liscaf", about = "Simple scaffolder using inquire")]
struct Args {
    /// New project name (used to replace template tokens)
    new_name: String,

    /// GitHub repo URL (HTTPS). Example: https://github.com/owner/repo
    repo_url: Option<String>,
    /// If set, show planned changes but don't write files or initialize git
    #[arg(long)]
    dry_run: bool,
    /// Assume yes to all prompts (non-interactive)
    #[arg(short = 'y', long = "yes")]
    yes: bool,
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
        repo_url = Text::new("Enter repository HTTPS URL:")
            .with_placeholder("https://github.com/owner/repo")
            .prompt()?;
    } else if !assume_yes {
        if !Confirm::new(&format!("Use repo URL '{}' ?", repo_url))
            .with_default(true)
            .prompt()? {
            repo_url = Text::new("Enter repository HTTPS URL:")
                .with_placeholder("https://github.com/owner/repo")
                .prompt()?;
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

    if !Confirm::new(&format!("Proceed to scaffold '{}'
from '{}' replacing '{}' ?", new_name, repo_url, template_base))
        .with_default(true)
        .prompt()? {
        println!("Aborted by user.");
        return Ok(());
    }

    let dry_run = args.dry_run;
    // Run scaffold (synchronous, prints to stdout)
    run_scaffold(&repo_url, &new_name, &template_base, dry_run)?;

    Ok(())
}

fn run_scaffold(repo_url: &str, new_name: &str, template_base: &str, dry_run: bool) -> anyhow::Result<()> {
    println!("Starting scaffolding for '{}'", new_name);
    println!("Repo URL: {}", repo_url);

    if !repo_url.starts_with("https://") {
        anyhow::bail!("Repo URL must start with https://");
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
