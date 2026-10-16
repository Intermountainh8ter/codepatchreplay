//! codepatchreplay CLI.
//!
//! `score` — compute pass@k over a generations JSONL, 95% bootstrap CI.
//! `audit` — build a full ReplicationReport against a paper claim.
//!   Exit 0 Scored, 1 TasksBelowMinimum / TemplateDrift.

use clap::{Parser, Subcommand};
use codepatchreplay::{
    build_report, pass_at_k_bootstrap_ci, per_task_pass_at_k, GeneratedPatch, ReportVerdict,
    RunManifest,
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "codepatchreplay",
    version,
    about = "Pre-registered code-generation reproducibility"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Compute pass@k mean + 95% bootstrap CI over a generations JSONL.
    Score {
        generations: PathBuf,
        #[arg(long, default_value_t = 2000)]
        boot: usize,
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
    /// Build a full ReplicationReport.
    Audit {
        #[arg(long)]
        paper_id: String,
        #[arg(long)]
        paper_claim: f64,
        #[arg(long)]
        paper_chat_template_sha256_hex: String,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        generations: PathBuf,
        /// Optional path to a reference-patches JSONL (`{"patch_text": ...}`).
        #[arg(long)]
        reference_patches: Option<PathBuf>,
        #[arg(long, default_value_t = 2000)]
        boot: usize,
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
    /// Print the version banner and exit.
    Version,
}

fn read_jsonl<T: serde::de::DeserializeOwned>(p: &PathBuf) -> anyhow::Result<Vec<T>> {
    let text = std::fs::read_to_string(p)?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        out.push(serde_json::from_str(t).map_err(|e| anyhow::anyhow!("line {}: {e}", i + 1))?);
    }
    Ok(out)
}

#[derive(serde::Deserialize)]
struct RefRow {
    patch_text: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Score {
            generations,
            boot,
            seed,
        } => {
            let gens: Vec<GeneratedPatch> = read_jsonl(&generations)?;
            let scores = per_task_pass_at_k(&gens);
            if scores.is_empty() {
                anyhow::bail!("no tasks in generations file");
            }
            let (mean, lo, hi) = pass_at_k_bootstrap_ci(&scores, boot, seed);
            println!("pass@k      = {mean:.4}");
            println!("95% CI      = [{lo:.4}, {hi:.4}]  (boot={boot}, seed={seed}, n_tasks={})", scores.len());
        }
        Cmd::Audit {
            paper_id,
            paper_claim,
            paper_chat_template_sha256_hex,
            manifest,
            generations,
            reference_patches,
            boot,
            seed,
        } => {
            let manifest: RunManifest =
                serde_json::from_str(&std::fs::read_to_string(&manifest)?)?;
            let gens: Vec<GeneratedPatch> = read_jsonl(&generations)?;
            let refs: Vec<String> = match reference_patches {
                None => Vec::new(),
                Some(p) => {
                    let rows: Vec<RefRow> = read_jsonl(&p)?;
                    rows.into_iter().map(|r| r.patch_text).collect()
                }
            };
            let r = build_report(
                &paper_id,
                paper_claim,
                &paper_chat_template_sha256_hex,
                manifest,
                &gens,
                &refs,
                boot,
                seed,
            );
            println!("{}", serde_json::to_string_pretty(&r)?);
            match r.verdict {
                ReportVerdict::Scored(_) => {}
                _ => std::process::exit(1),
            }
        }
        Cmd::Version => println!("codepatchreplay {}", env!("CARGO_PKG_VERSION")),
    }
    Ok(())
}
