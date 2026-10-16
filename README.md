# codepatchreplay

**Pre-registered code-generation reproducibility.** Pin the target,
chat template, sampling, test-map SHA, and k; score pass@k with a
95% bootstrap CI; flag byte-identical patches against a reference
set as a training-data leak signal.

![status: initial scaffold](https://img.shields.io/badge/status-initial%20scaffold-lightgrey)
![language: Rust](https://img.shields.io/badge/language-Rust-blue)
![license: MIT](https://img.shields.io/badge/license-MIT-blue)

## Why

HumanEval has been patched twice. MBPP has two public test maps. SWE-bench's
scaffolding changes between releases. "pass@1 = 0.84" is routinely
compared across rows that pin none of this, and the comparison is
sometimes wrong by a factor of two. codepatchreplay is the manifest
layer plus the pass@k scorer, so the row that says "I replicated" has
a committed hash the paper's row either shares or doesn't.

The leak signal — fraction of generated patches byte-identical (modulo
whitespace) to a committed reference set — is here because a
code-gen row with high pass@k and high patch-identity is answering by
memorization, and that distinction belongs on the same report.

## What is in this scaffold

- `RunManifest { benchmark, target, chat_template_sha256_hex, sampling, test_map_sha256_hex, k }`
- `canonical_patch` / `patch_sha256_hex` — whitespace-normalized hash.
- `per_task_pass_at_k` — per-task pass@k scores (0 or 1), sorted by task_id for reproducible bootstraps.
- `pass_at_k_bootstrap_ci` — 95% percentile CI, Pcg64Mcg.
- `patch_identity_rate` — fraction of generations whose canonical SHA
  appears in a reference set.
- `build_report(...)` — full `ReplicationReport` with the pre-reg §6
  verdict (TemplateDrift / TasksBelowMinimum / Scored(ReplicationVerdict)).
- CLI: `score`, `audit`, `version`. `audit` exits 0 on `Scored`, 1 otherwise.

## What is openly deferred (§8)

- No live test runner. The scaffold reads a generations JSONL with
  pre-computed `passes_tests` bits.
- No live sampler. Pair with a separate driver (or
  `jailbreakreplay::http::sample_many` for OpenAI-schema targets).
- No semantic patch equivalence. The leak signal is byte-identity
  (modulo whitespace); near-copies don't surface today.

## Related

Part of the [bench](https://github.com/Intermountainh8ter?tab=repositories)
suite of pre-registered audit crates.

## License

MIT.
