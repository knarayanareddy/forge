//! Download HF/GGUF weights referenced by `models/registry.toml`.
//!
//! Stages files on disk only — mlx/gguf inference is not wired yet (REG-01 / MLX-01).

use aether_core::{discover_registry_path, download_file, DownloadPlan, ModelRegistry, ENV_REGISTRY_PATH};
use std::env;
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!(
        "Usage: aether-download-model --profile <id> [--registry <path>] [--repo-root <path>]\n\
         \n\
         Downloads hf_repo/hf_file into the profile model_path. GGUF profiles with hf_file work today;\n\
         MLX bundles may need manual placement until multi-artifact sync lands."
    );
    std::process::exit(2);
}

fn parse_args() -> (String, PathBuf, PathBuf) {
    let mut profile = None;
    let mut registry = None;
    let mut repo_root = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let args: Vec<String> = env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--profile" => {
                i += 1;
                profile = Some(args.get(i).cloned().unwrap_or_else(|| usage()));
            }
            "--registry" => {
                i += 1;
                registry = Some(PathBuf::from(args.get(i).cloned().unwrap_or_else(|| usage())));
            }
            "--repo-root" => {
                i += 1;
                repo_root = PathBuf::from(args.get(i).cloned().unwrap_or_else(|| usage()));
            }
            "-h" | "--help" => usage(),
            other => {
                eprintln!("Unknown argument: {other}");
                usage();
            }
        }
        i += 1;
    }
    let profile = profile.unwrap_or_else(|| usage());
    let registry = registry
        .or_else(discover_registry_path)
        .unwrap_or_else(|| repo_root.join("models/registry.toml"));
    (profile, registry, repo_root)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (profile_id, registry_path, repo_root) = parse_args();
    if !registry_path.is_file() {
        return Err(format!("registry not found: {}", registry_path.display()).into());
    }
    if env::var(ENV_REGISTRY_PATH).is_err() {
        env::set_var(ENV_REGISTRY_PATH, registry_path.to_string_lossy().as_ref());
    }

    let registry = ModelRegistry::load_from_path(&registry_path)?;
    let profile = registry.profile(&profile_id)?;
    let plan = DownloadPlan::from_profile(&repo_root, profile)?;
    println!("Downloading {} -> {}", plan.hf_repo, plan.dest.display());
    let dest = download_file(&plan).await?;
    println!("Saved {}", dest.display());
    println!(
        "Note: inference for backend {:?} is not wired yet; weights are staged only.",
        profile.backend
    );
    Ok(())
}
