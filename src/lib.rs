//! Pre-registered code-generation reproducibility.
//!
//! Code-gen benchmarks (HumanEval, MBPP, SWE-bench) report pass@k —
//! "k samples per task, task passes if at least one sample passes the
//! held-out tests." Published numbers routinely diverge on
//! replication: the paper used a different sampler, a different k, a
//! different test harness, or (in the worst case) the training data
//! contained one of the reference solutions. This crate pins all
//! four and adds a canary-diff signal — the fraction of generated
//! patches that are byte-identical to a committed reference set —
//! so a reviewer can see memorization alongside pass@k.
//!
//! See `docs/PREREGISTRATION.md` for the clauses this scaffold closes.

use rand::SeedableRng;
use rand_pcg::Pcg64Mcg;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Pre-reg §2 — the target identity.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Target {
    pub id: String,
    pub revision: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Sampling {
    pub temperature: f64,
    pub top_p: f64,
    pub top_k: u32,
    pub max_tokens: u32,
    pub seed: u64,
}

/// Pre-reg §2 pinned fields for a code-gen run.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RunManifest {
    pub benchmark: String,
    pub target: Target,
    pub chat_template_sha256_hex: String,
    pub sampling: Sampling,
    /// SHA of the task_id → test map file. A different map is a
    /// different benchmark (even at the same name).
    pub test_map_sha256_hex: String,
    /// Samples drawn per task. `k` in pass@k.
    pub k: usize,
}

/// One generated sample for one task.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeneratedPatch {
    pub task_id: String,
    /// The patch text itself. Unified-diff format expected but not
    /// enforced; the comparison uses `canonical_patch` which
    /// whitespace-normalizes.
    pub patch_text: String,
    /// Whether the task's held-out tests pass when this patch is
    /// applied. Caller computes this; the library trusts the bit and
    /// scores pass@k from it.
    pub passes_tests: bool,
}

/// Whitespace-normalized form of a patch for byte comparison.
/// Collapses runs of whitespace to single spaces, trims leading and
/// trailing whitespace. Case-sensitive — code identity matters.
pub fn canonical_patch(patch: &str) -> String {
    let mut out = String::with_capacity(patch.len());
    let mut last_was_space = true; // trims leading
    for c in patch.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(c);
            last_was_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// SHA-256 of a canonical patch, lowercase hex.
pub fn patch_sha256_hex(patch: &str) -> String {
    let c = canonical_patch(patch);
    let d = Sha256::digest(c.as_bytes());
    let mut s = String::with_capacity(d.len() * 2);
    for b in d {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// pass@k over a task's `k` samples: 1.0 if at least one passes, else
/// 0.0. The `samples` slice is expected to be exactly k samples for
/// one task; the caller aggregates across tasks.
pub fn task_pass_at_k(samples: &[GeneratedPatch]) -> f64 {
    if samples.iter().any(|s| s.passes_tests) {
        1.0
    } else {
        0.0
    }
}

/// pass@k across a benchmark: group `generations` by task_id and
/// compute per-task pass@k, then return the per-task scores (0 or 1
/// each) in task-id-sorted order so bootstrap CIs are reproducible.
pub fn per_task_pass_at_k(generations: &[GeneratedPatch]) -> Vec<f64> {
    let mut by_task: std::collections::BTreeMap<&str, Vec<&GeneratedPatch>> =
        std::collections::BTreeMap::new();
    for g in generations {
        by_task.entry(g.task_id.as_str()).or_default().push(g);
    }
    by_task
        .into_values()
        .map(|v| {
            if v.iter().any(|s| s.passes_tests) {
                1.0
            } else {
                0.0
            }
        })
        .collect()
}

/// Fraction of generated patches whose canonical SHA appears in the
/// reference set. Pre-reg §5 — if the training data contained one of
/// the reference solutions, byte-identical patches surface as a leak
/// signal.
pub fn patch_identity_rate(
    generations: &[GeneratedPatch],
    reference_patches: &[String],
) -> f64 {
    if generations.is_empty() {
        return 0.0;
    }
    let refs: std::collections::HashSet<String> = reference_patches
        .iter()
        .map(|p| patch_sha256_hex(p))
        .collect();
    let hits = generations
        .iter()
        .filter(|g| refs.contains(&patch_sha256_hex(&g.patch_text)))
        .count();
    hits as f64 / generations.len() as f64
}

/// Mean + 95% percentile-bootstrap CI across per-task pass@k scores.
pub fn pass_at_k_bootstrap_ci(scores: &[f64], iters: usize, seed: u64) -> (f64, f64, f64) {
    assert!(!scores.is_empty());
    let mut rng = Pcg64Mcg::seed_from_u64(seed);
    let mut means = Vec::with_capacity(iters);
    let n = scores.len();
    for _ in 0..iters {
        let mut sum = 0.0;
        for _ in 0..n {
            let k = (rand::Rng::gen::<u32>(&mut rng) as usize) % n;
            sum += scores[k];
        }
        means.push(sum / n as f64);
    }
    means.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mean = scores.iter().sum::<f64>() / n as f64;
    let lo = means[(iters as f64 * 0.025).floor() as usize];
    let hi = means[((iters as f64 * 0.975).ceil() as usize).min(iters - 1)];
    (mean, lo, hi)
}

/// Pre-reg §4 minimum trials.
pub const MIN_TASKS: usize = 50;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReplicationVerdict {
    Reproduced,
    NarrowedBelowClaim,
    StrongerThanClaim,
    Diverged,
}

pub fn verdict_vs_claim(claim: f64, ci: (f64, f64)) -> ReplicationVerdict {
    let (lo, hi) = ci;
    if lo <= claim && claim <= hi {
        ReplicationVerdict::Reproduced
    } else if hi < claim {
        ReplicationVerdict::NarrowedBelowClaim
    } else if lo > claim {
        ReplicationVerdict::StrongerThanClaim
    } else {
        ReplicationVerdict::Diverged
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReportVerdict {
    /// §4 blocks: too few tasks (N < MIN_TASKS).
    TasksBelowMinimum,
    /// §2 blocks: paper chat_template SHA != replication's.
    TemplateDrift,
    /// Scored cleanly.
    Scored(ReplicationVerdict),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplicationReport {
    pub paper_id: String,
    pub paper_claim_pass_at_k: f64,
    pub manifest: RunManifest,
    pub n_tasks: usize,
    pub pass_at_k_mean: f64,
    pub pass_at_k_ci_lo: f64,
    pub pass_at_k_ci_hi: f64,
    pub patch_identity_rate: f64,
    pub verdict: ReportVerdict,
}

#[allow(clippy::too_many_arguments)]
pub fn build_report(
    paper_id: &str,
    paper_claim_pass_at_k: f64,
    paper_chat_template_sha256_hex: &str,
    manifest: RunManifest,
    generations: &[GeneratedPatch],
    reference_patches: &[String],
    boot_iters: usize,
    boot_seed: u64,
) -> ReplicationReport {
    let scores = per_task_pass_at_k(generations);
    let n_tasks = scores.len();
    let (mean, lo, hi) = if scores.is_empty() {
        (f64::NAN, f64::NAN, f64::NAN)
    } else {
        pass_at_k_bootstrap_ci(&scores, boot_iters, boot_seed)
    };
    let identity = patch_identity_rate(generations, reference_patches);
    let verdict = if paper_chat_template_sha256_hex != manifest.chat_template_sha256_hex {
        ReportVerdict::TemplateDrift
    } else if n_tasks < MIN_TASKS {
        ReportVerdict::TasksBelowMinimum
    } else {
        ReportVerdict::Scored(verdict_vs_claim(paper_claim_pass_at_k, (lo, hi)))
    };
    ReplicationReport {
        paper_id: paper_id.into(),
        paper_claim_pass_at_k,
        manifest,
        n_tasks,
        pass_at_k_mean: mean,
        pass_at_k_ci_lo: lo,
        pass_at_k_ci_hi: hi,
        patch_identity_rate: identity,
        verdict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sampling() -> Sampling {
        Sampling {
            temperature: 0.2,
            top_p: 0.95,
            top_k: 50,
            max_tokens: 1024,
            seed: 1,
        }
    }

    fn manifest() -> RunManifest {
        RunManifest {
            benchmark: "humaneval".into(),
            target: Target {
                id: "org/model".into(),
                revision: "r1".into(),
            },
            chat_template_sha256_hex: "SHA".into(),
            sampling: sampling(),
            test_map_sha256_hex: "map-sha".into(),
            k: 1,
        }
    }

    #[test]
    fn canonical_patch_collapses_whitespace() {
        let a = "  def f():\n    return 1\n\n";
        let b = "def f():\treturn 1";
        assert_eq!(canonical_patch(a), canonical_patch(b));
    }

    #[test]
    fn patch_sha256_is_lowercase_hex() {
        let h = patch_sha256_hex("def f(): return 1");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn task_pass_at_k_is_any_passes() {
        let s = vec![
            GeneratedPatch {
                task_id: "t".into(),
                patch_text: "x".into(),
                passes_tests: false,
            },
            GeneratedPatch {
                task_id: "t".into(),
                patch_text: "y".into(),
                passes_tests: true,
            },
        ];
        assert_eq!(task_pass_at_k(&s), 1.0);
    }

    #[test]
    fn per_task_pass_at_k_sorts_by_task_id() {
        let gens = vec![
            GeneratedPatch {
                task_id: "b".into(),
                patch_text: "x".into(),
                passes_tests: true,
            },
            GeneratedPatch {
                task_id: "a".into(),
                patch_text: "x".into(),
                passes_tests: false,
            },
            GeneratedPatch {
                task_id: "a".into(),
                patch_text: "y".into(),
                passes_tests: false,
            },
        ];
        // a: no pass → 0.0, b: one pass → 1.0, sorted order [a,b] → [0, 1].
        assert_eq!(per_task_pass_at_k(&gens), vec![0.0, 1.0]);
    }

    #[test]
    fn patch_identity_rate_flags_byte_identical_reference() {
        let gens = vec![
            GeneratedPatch {
                task_id: "a".into(),
                patch_text: "def solution(): return 42".into(),
                passes_tests: true,
            },
            GeneratedPatch {
                task_id: "b".into(),
                patch_text: "def unique(): return 7".into(),
                passes_tests: true,
            },
        ];
        let refs = vec!["  def   solution():   return 42  ".to_string()];
        // Whitespace-normalization makes the first match; the second does not.
        assert!((patch_identity_rate(&gens, &refs) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn patch_identity_rate_is_zero_on_empty_generations() {
        assert_eq!(patch_identity_rate(&[], &vec!["x".into()]), 0.0);
    }

    #[test]
    fn verdict_vs_claim_matches_four_regions() {
        assert_eq!(
            verdict_vs_claim(0.5, (0.4, 0.6)),
            ReplicationVerdict::Reproduced
        );
        assert_eq!(
            verdict_vs_claim(0.9, (0.4, 0.6)),
            ReplicationVerdict::NarrowedBelowClaim
        );
        assert_eq!(
            verdict_vs_claim(0.1, (0.4, 0.6)),
            ReplicationVerdict::StrongerThanClaim
        );
    }

    #[test]
    fn build_report_blocks_on_template_drift() {
        let gens: Vec<_> = (0..60)
            .map(|i| GeneratedPatch {
                task_id: format!("t{i}"),
                patch_text: "x".into(),
                passes_tests: true,
            })
            .collect();
        let r = build_report("paper", 0.9, "paper-sha", manifest(), &gens, &[], 100, 1);
        assert_eq!(r.verdict, ReportVerdict::TemplateDrift);
    }

    #[test]
    fn build_report_blocks_on_tasks_below_minimum() {
        let gens: Vec<_> = (0..10)
            .map(|i| GeneratedPatch {
                task_id: format!("t{i}"),
                patch_text: "x".into(),
                passes_tests: true,
            })
            .collect();
        let r = build_report("paper", 0.9, "SHA", manifest(), &gens, &[], 100, 1);
        assert_eq!(r.verdict, ReportVerdict::TasksBelowMinimum);
    }

    #[test]
    fn build_report_scores_cleanly_above_minimum() {
        let gens: Vec<_> = (0..100)
            .map(|i| GeneratedPatch {
                task_id: format!("t{i}"),
                patch_text: format!("solution {i}"),
                passes_tests: i % 2 == 0,
            })
            .collect();
        let r = build_report("paper", 0.5, "SHA", manifest(), &gens, &[], 500, 1);
        assert!(matches!(r.verdict, ReportVerdict::Scored(_)));
        assert_eq!(r.n_tasks, 100);
        // About half pass.
        assert!(r.pass_at_k_mean > 0.4 && r.pass_at_k_mean < 0.6);
    }

    #[test]
    fn report_round_trips_json() {
        let gens: Vec<_> = (0..60)
            .map(|i| GeneratedPatch {
                task_id: format!("t{i}"),
                patch_text: "x".into(),
                passes_tests: true,
            })
            .collect();
        let r = build_report("p", 1.0, "SHA", manifest(), &gens, &[], 100, 1);
        let s = serde_json::to_string(&r).unwrap();
        let r2: ReplicationReport = serde_json::from_str(&s).unwrap();
        assert_eq!(r.verdict, r2.verdict);
    }
}
