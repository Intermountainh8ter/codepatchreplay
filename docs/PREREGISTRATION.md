# codepatchreplay pre-registration

Locked before the first code-gen reproducibility result is committed.
Append-only.

## 1. Scope

The population under audit is **code-generation benchmark rows**:
HumanEval, MBPP, SWE-bench, LiveCodeBench, and similar — a (target,
chat template, sampling, k) tuple scored against a committed
task_id → test map. codepatchreplay does not run tests; a caller
supplies the pass/fail bit per sample. This crate pins the manifest
and scores pass@k + a byte-identity leak signal.

## 2. The pinned fields

A `RunManifest` pins:

1. **Benchmark name** (string) — e.g. `"humaneval"`.
2. **Target** — `(id, revision)`. A different revision is a different
   target.
3. **Chat template SHA** — at run time. Template drift between paper
   and replication blocks scoring (§6).
4. **Sampling** — `(temperature, top_p, top_k, max_tokens, seed)`.
5. **test_map_sha256_hex** — SHA of the task_id → test file map. A
   different map is a different benchmark (even at the same name;
   HumanEval has been patched at least twice).
6. **k** — samples per task.

## 3. Patch canonicalization

Patches are compared via `canonical_patch`: whitespace runs collapsed
to single spaces, leading/trailing whitespace trimmed, case
preserved. Code identity matters; a reformatter is a different patch.
Two patches with the same `patch_sha256_hex` are byte-identical under
this canonicalization.

## 4. pass@k

Per-task pass@k is 1.0 if at least one of the task's k samples passes
the held-out tests, else 0.0. Across tasks, the mean carries a 95%
percentile-bootstrap CI resampling tasks with replacement (Pcg64Mcg,
same recipe as the other crates).

Minimum `n_tasks` for a scored verdict is `MIN_TASKS = 50`. A
replication with fewer tasks blocks on §6's `TasksBelowMinimum`.

## 5. Patch-identity leak signal

`patch_identity_rate` is the fraction of generated patches whose
canonical SHA appears in a committed reference-patch set (a snapshot
of the benchmark's reference solutions, or a training-data subset).
A high rate alongside a high pass@k is the training-data-leak
signature: the model is answering by memorization, not by solving.
The pre-reg reports this number alongside pass@k; a release gate on
it is a separate `releaseflag` configuration.

## 6. Verdict

Three outcomes, read from `ReportVerdict`:

- **TemplateDrift** — paper's chat template SHA ≠ replication's. The
  row is non-comparable; score is not reported.
- **TasksBelowMinimum** — n_tasks < 50. Score is not reported.
- **Scored(ReplicationVerdict)** — scored. The inner verdict is one
  of Reproduced / NarrowedBelowClaim / StrongerThanClaim / Diverged,
  matching the jailbreakreplay convention: Reproduced if the paper's
  claim is inside the replication's 95% CI, Narrowed if the CI is
  strictly below, Stronger if strictly above, Diverged otherwise.

## 7. What this crate does NOT do

- Run tests. The caller supplies the pass/fail bit; this crate scores
  the aggregated metric.
- Decide what the "right" test harness is. The test map SHA pins
  whichever harness the manifest declares; different harnesses are
  different rows.
- Rewrite patches. `canonical_patch` normalizes whitespace only;
  anything else is a different patch.

## 8. Openly tracked gaps

- No live test runner. A future `codepatchreplay observe
  --target http://... --tests ./tests/` would spawn the target and
  run its generations through a sandboxed test harness.
- No live sampler. The scaffold reads a generations JSONL the caller
  produced; a future `codepatchreplay sample` would wrap the OpenAI
  chat API with the pinned sampling.
- No semantic equivalence check. A patch that solves the problem
  with different code is counted on `passes_tests`, which is right;
  but the leak signal only flags byte-identical copies, not
  near-copies. A future AST-level or test-isomorphism signal would
  land here.
- No cross-crate manifest import. codepatchreplay's `RunManifest`
  is a near-twin of harnesspin's; a lifter `harnesspin::from_codepatchreplay`
  would make the suite more cohesive.
