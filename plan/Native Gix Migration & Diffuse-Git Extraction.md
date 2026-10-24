# Native Gix Migration & Diffuse-Git Crate Extraction

- [x] **Phase 1 — `diffuse-parse` AST Extraction & `diffuse-git` Autonomous Crate Architecture**
- [ ] **Phase 2 — Pure-Gix Object Database, Blob Loading & Multi-Threaded Cache**
- [ ] **Phase 3 — Native Working Tree Status, Dirwalk & Whitespace Reconciliation**
- [ ] **Phase 4 — In-Process Unified Diff Engine & Streaming Producer**
- [ ] **Phase 5 — Native Index Mutation: Staging, Unstaging, Discard & Safety Stash**
- [ ] **Phase 6 — Comparative Stress Profiling Suite (`gix` vs `git`) & Subprocess Evaluation**

---

## Executive Summary

`diffuse` currently relies on invoking the `git` binary as an external subprocess across several foundational workflows: unified diff generation (`git diff -U4 -M -w`), untracked file enumeration (`git ls-files --others`), name status query (`git diff --name-status`), batch blob retrieval fallback (`git cat-file --batch`), commit revision resolution (`git rev-list`, `git rev-parse`), and TUI interactive operations (`git apply`, `git add`, `git reset`, `git checkout`, `git stash create`).

This architecture creates four core limitations:
1. **Host Environment Fragility**: In environments where `git` is absent from `PATH` (such as minimal container environments, embedded execution, scratch containers, or constrained worker pools), `diffuse` fails immediately on repository initialization.
2. **Subprocess Spawn Latency**: Spawning processes, piping buffers across standard I/O streams, and tearing down child processes imposes 5–25 ms of operating system latency per invocation, noticeably degrading responsiveness during rapid navigation, file switching, and multi-file staging.
3. **Layer Coupling & Duplication**: Git interaction code is currently split between `crates/diffuse-core/src/git.rs`, `crates/diffuse-core/src/repo.rs`, and `crates/diffuse-tui/src/actions.rs`.
4. **Temporary File Leakage**: Patch application for hunk staging and discarding currently writes intermediate patch text to disk via `tempfile`, requiring file system write permissions and garbage collection.

### Core Architectural Invariants

1. **Autonomous Crate Hierarchy with `diffuse-parse`**:
   - Extract `diffuse-parse` as a zero-git, zero-analysis foundation containing the canonical diff AST and parser (`DiffFile`, `Hunk`, `Row`, `RowKind`, `FileStatus`, `parse_diff()`, `zero_hunk_section()`). Plain piped diffs (`cat patch | diffuse`) use `diffuse-parse` directly without loading git.
   - `diffuse-git` depends only on `diffuse-parse` and `gix`. It has **zero dependencies** on any downstream diffuse crates (`diffuse-core`, `diffuse-algo`, `diffuse-syntax`, `diffuse-render`, `diffuse-tui`).
   - The resulting unidirectional workspace dependency graph is:
     $$\begin{matrix}
     & & \text{diffuse-parse} & & \\
     & \swarrow & & \searrow & \\
     \text{diffuse-git} & & & & \text{diffuse-core} \\
     & \searrow & & \swarrow & \\
     & & \text{diffuse-render} & & \\
     & & \downarrow & & \\
     & & \text{diffuse-tui} & & \\
     & & \downarrow & & \\
     & & \text{diffuse} & &
     \end{matrix}$$
2. **Canonical Trim-Equality Whitespace Diffing**:
   - `git diff -w` (`--ignore-all-space`) collapses *all* whitespace, including internal spaces between tokens (`foo bar` conflated with `foobar` or `foo   bar`). This blunt suppression forced the engine to build complex post-hoc resurfacing passes to recover missing hunks and zero-hunk files.
   - `gix` enables programmatic line hashing and token comparison. `diffuse-git` implements diffuse's exact whitespace invariant: `trimmed_id == trimmed_id` (ignoring leading indentation and trailing whitespace, while strictly preserving internal whitespace and syntactic token separation).
   - Reindented blocks match cleanly without losing internal whitespace modifications or requiring synthetic tape surgery.
3. **Pure-Gix First Strategy**: Build `diffuse-git` exclusively with native `gix` (Gitoxide) primitives across all operations. Avoid building redundant subprocess wrappers or dual-execution branches prematurely.
4. **Empirical Profiling Gate for Subprocess Usage**: Subprocess execution will **not** be admitted into `diffuse-git` unless targeted stress benchmarks against heavy repositories demonstrate that git CLI is radically faster on a specific hot path. If `gix` is roughly on par or faster (as expected for in-process memory-mapped operations), `diffuse-git` remains 100% pure Rust without subprocess code.
5. **Zero-Tempfile Index & Worktree Mutations**: Staging, unstaging, and discarding operate directly on the memory-mapped `gix::index::State` and working tree files, without creating intermediate patch files on disk.

---

## Table of Contents

1. [Phase 1 — `diffuse-parse` AST Extraction & `diffuse-git` Autonomous Crate Architecture](#1-phase-1--diffuse-parse-ast-extraction--diffuse-git-autonomous-crate-architecture)
2. [Phase 2 — Pure-Gix Object Database, Blob Loading & Multi-Threaded Cache](#2-phase-2--pure-gix-object-database-blob-loading--multi-threaded-cache)
3. [Phase 3 — Native Working Tree Status, Dirwalk & Whitespace Reconciliation](#3-phase-3--native-working-tree-status-dirwalk--whitespace-reconciliation)
4. [Phase 4 — In-Process Unified Diff Engine & Streaming Producer](#4-phase-4--in-process-unified-diff-engine--streaming-producer)
5. [Phase 5 — Native Index Mutation: Staging, Unstaging, Discard & Safety Stash](#5-phase-5--native-index-mutation-staging-unstaging-discard--safety-stash)
6. [Phase 6 — Comparative Stress Profiling Suite (`gix` vs `git`) & Subprocess Evaluation](#6-phase-6--comparative-stress-profiling-suite-gix-vs-git--subprocess-evaluation)

---

## 1. Phase 1 — `diffuse-parse` AST Extraction & `diffuse-git` Autonomous Crate Architecture

Decouple the unified diff grammar and AST from `diffuse-core` into `diffuse-parse`, and establish `diffuse-git` as an autonomous workspace crate.

### Step 1.1: Extract `crates/diffuse-parse` Workspace Crate
- **Location**: `crates/diffuse-parse/Cargo.toml`, `crates/diffuse-parse/src/lib.rs`
- Extract parsing structures and routines currently in `crates/diffuse-core/src/parse.rs`:
  - Diff row hierarchy: `RowKind`, `RowGroup`, `DelLine`, `AddLine`, `CtxLine`, `Row`, `Hunk`.
  - File model: `FileStatus`, `DiffFile`, `zero_hunk_section`.
  - Parser routines: `parse_diff`, `del_count`, line counting helpers.
- Dependencies: `serde`, `diffuse-algo` (for validation contracts `require!`), `smallvec`.
- Invariant: Zero dependencies on `gix`, `diffuse-core`, or git concepts. This crate serves plain piped diffs and git diffs alike.

### Step 1.2: Create `crates/diffuse-git` Workspace Crate
- **Location**: `crates/diffuse-git/Cargo.toml`, `Cargo.toml`
- Create `crates/diffuse-git` as a Cargo workspace member.
- Upgrade and configure `gix` in `Cargo.toml` workspace dependencies:
  - Version: `0.90` (or latest stable available).
  - Features: `["basic", "revision", "blob-diff", "parallel", "sha1", "dirwalk", "index", "status", "gix-status", "worktree-mutation", "attributes"]`.
- `crates/diffuse-git/Cargo.toml` dependencies:
  - `diffuse-parse` (workspace dependency)
  - `gix` (workspace dependency)
  - `smallvec` (workspace dependency)
  - `tracing` (workspace dependency)
  - `tempfile` (workspace dependency, dev-dependencies only for tests)
- Invariant: Zero dependencies on `diffuse-core`, `diffuse-syntax`, `diffuse-render`, `diffuse-tui`.

### Step 1.3: Define Canonical Git Domain Types in `diffuse-git`
- **Location**: `crates/diffuse-git/src/types.rs`, `crates/diffuse-git/src/lib.rs`
- Define public domain types:
  ```rust
  pub struct Repo {
      root: Option<PathBuf>,
      name: Option<String>,
  }

  pub struct ReviewTarget {
      pub repo: Repo,
      pub range: Option<RevisionRange>,
      pub git_args: Vec<String>,
      pub rev_args: Vec<String>,
      pub pathspecs: Vec<String>,
      pub staged: bool,
  }

  pub struct RevisionRange(pub String, pub String);

  pub struct RefPair {
      pub worktree_new: bool,
      pub new_ref: Option<String>,
      pub old_ref: Option<String>,
  }

  pub struct GitBlob {
      pub data: Vec<u8>,
      pub size: usize,
      pub is_binary: bool,
  }

  pub struct GitDiffOptions {
      pub context: usize,
      pub rename_limit: Option<usize>,
      pub ignore_whitespace: bool,
      pub pathspecs: Vec<String>,
  }
  ```
- Implement pathspec and revision arg parsing (`parse_git_rev_and_pathspecs`).
- Test criteria: Unit tests in `crates/diffuse-git/src/types/tests.rs` verifying string representations, revision range parsing, and target construction without repository access.

### Step 1.4: Expose Unified `Repository` Facade in `diffuse-git`
- **Location**: `crates/diffuse-git/src/repository.rs`
- Define `pub struct Repository`:
  - `pub fn discover(cwd: &Path) -> Result<Self, GitError>`
  - `pub fn open(root: &Path) -> Result<Self, GitError>`
  - `pub fn root(&self) -> &Path`
  - `pub fn name(&self) -> &str`
  - `pub fn current_branch(&self) -> Option<String>`
  - `pub fn head_commit(&self) -> Result<String, GitError>`
  - `pub fn resolve_refs(&self, base_args: &[String], staged: bool) -> Result<RefPair, GitError>`
  - `pub fn resolve_commit_range(&self, rev_args: &[String]) -> Result<Option<String>, GitError>`

---

## 2. Phase 2 — Pure-Gix Object Database, Blob Loading & Multi-Threaded Cache

Eliminate `git cat-file --batch` and all subprocesses for historical object and tree inspection.

### Step 2.1: Native Tree Traversal & Path Lookup
- **Location**: `crates/diffuse-git/src/objects.rs`
- Implement tree caching and path resolution via `gix::Repository`:
  - Resolve arbitrary ref expressions (`HEAD`, `HEAD~1`, `main`, tags, short and long commit hashes) using `repo.rev_parse_single`.
  - Peel objects to target commits and root trees.
  - Traverse trees by path using `tree.lookup_entry_by_path(&path)`.
  - Handle subtrees and nested paths efficiently using path traversal without recursive tree allocations.

### Step 2.2: Zero-Copy Blob Retrieval & Binary Detection
- **Location**: `crates/diffuse-git/src/objects.rs`
- Implement `load_blob(&self, ref_str: &str, path: &str) -> Result<GitBlob, GitError>`:
  - Fetch blob directly from packfile or loose object storage.
  - Determine binary content by inspecting first 8,000 bytes for null bytes (`\0`).
  - Extract UTF-8 text where valid, preserving byte vectors for binary assets.
- Implement batch loading `load_blobs_batch(&self, specs: &[BlobSpec]) -> HashMap<BlobSpec, GitBlob>`:
  - Group specs by tree commit to reuse parsed trees.
  - Parallelize blob extraction across multiple threads using `gix` thread-safe object database handles.
  - Zero subprocess fallback: packfile search is 100% native in Rust.

### Step 2.3: Integrate into `diffuse-core`
- **Location**: `crates/diffuse-core/src/git.rs` (delegating to `diffuse_git`), `crates/diffuse-core/Cargo.toml`
- Add `diffuse-parse` and `diffuse-git` to workspace dependencies.
- Replace legacy `load_git_blobs_batch` and `load_git_blobs_native` with direct calls to `diffuse_git::Repository::load_blobs_batch`.
- Test criteria: Unit tests loading hundreds of blobs across historical commits in temporary test repositories, verifying that execution succeeds when `git` binary is simulated as absent.

---

## 3. Phase 3 — Native Working Tree Status, Dirwalk & Whitespace Reconciliation

Replace `git status`, `git diff --name-status`, and `git ls-files --others` with native Gitoxide worktree and index inspection.

### Step 3.1: Native Untracked File Discovery via `gix::dir::walk`
- **Location**: `crates/diffuse-git/src/status.rs`
- Implement untracked file discovery using `gix` directory walk:
  - Load `.gitignore` hierarchically from root to subdirectories, plus `.git/info/exclude` and core.excludesfile.
  - Filter excluded files automatically.
  - Honor pathspecs passed on the command line.
  - Check file contents for null bytes (`\0`) to flag untracked binary files.
  - Construct synthetic `DiffFile` instances using `diffuse_parse::DiffFile`.
  - Replace `git ls-files --others --exclude-standard` entirely.

### Step 3.2: Native Working Tree & Index Status
- **Location**: `crates/diffuse-git/src/status.rs`
- Implement `status(&self, target: &ReviewTarget) -> Result<Vec<DiffFile>, GitError>`:
  - Working tree mode (`target.worktree_new == true`):
    - When `target.staged == false`: compare index to working tree (unstaged modifications, deletions, untracked additions).
    - When `target.staged == true`: compare HEAD tree to index (staged modifications, additions, deletions, renames).
  - Commit range mode (`target.range.is_some()`):
    - Diff old commit tree against new commit tree using `gix::diff::tree`.
    - Detect renames using `gix-diff` similarity matching with configurable threshold.

### Step 3.3: Native Whitespace-Suppressed File Reconciliation
- **Location**: `crates/diffuse-git/src/status.rs`
- In commit range and worktree modes with whitespace-ignoring diffs (`-w`):
  - Identify files that changed between trees or between tree and worktree, but whose non-whitespace characters are identical.
  - Emit zero-hunk sections directly in memory without executing a secondary `git diff -M --name-only` subprocess.
- Test criteria: Verify equivalence with git CLI on repositories containing renames, deletions, mode changes, untracked files, and whitespace-only modifications.

---

## 4. Phase 4 — In-Process Unified Diff Engine & Streaming Producer

Generate unified diff tapes and `DiffFile` ASTs directly in process using `gix-diff` / `gix::diff::blob`.

### Step 4.1: Pure-Rust Unified Diff Generator with Trim-Equality Semantics
- **Location**: `crates/diffuse-git/src/diff.rs`
- Implement in-process unified diff generator:
  - Sequence alignment using `gix_diff::blob` (or `diffuse-algo` Myers/histogram primitives).
  - Configurable context lines (`-U{context}`).
  - **Canonical Trim-Equality Comparator**:
    - Hash and compare lines using `line.trim()` (matching diffuse DP's `trimmed_id == trimmed_id`).
    - Eliminates git CLI's destructive `-w` behavior that strips internal whitespace.
    - Preserves internal token spacing (`foo bar` vs `foo  bar`) while allowing reindented code blocks to align as verbatim twins.
    - Radically reduces the need for synthetic tape post-processing and zero-hunk suppression workarounds.
  - Output parsed `diffuse_parse::DiffFile` instances directly, avoiding text formatting followed by re-parsing where in-process models match.
  - Emit unified diff syntax headers matching the canonical diffuse engine contract when raw patch text is required.

### Step 4.2: Streaming Repository Diff Producer
- **Location**: `crates/diffuse-git/src/stream.rs`
- Implement `stream_repository`:
  - Enumerate changed files and untracked entries.
  - Progressively emit each file's metadata, diff AST, and pre/post content snapshots.
  - Allow callers in `diffuse-core` or CLI to route streamed files directly into the diffuse analysis worker pool.

---

## 5. Phase 5 — Native Index Mutation: Staging, Unstaging, Discard & Safety Stash

Eliminate `git apply`, `git add`, `git reset`, `git checkout`, and `git stash` subprocesses in `diffuse-tui`.

### Step 5.1: In-Memory Safety Stash Commit Creation
- **Location**: `crates/diffuse-git/src/mutation.rs`
- Implement `create_stash_checkpoint(&self, message: &str) -> Result<String, GitError>`:
  - Read active index and write a tree object to the object database (`gix::index::State::write_tree`).
  - Create a commit object pointing to current HEAD and index tree.
  - Return the commit hash string without invoking `git stash create`.
  - Zero filesystem side effects and zero reflog pollution unless configured.

### Step 5.2: Native Staging & Unstaging
- **Location**: `crates/diffuse-git/src/mutation.rs`
- Implement whole-file staging (`stage_file`):
  - Read working tree file, hash and store blob into ODB, update or create entry in `gix::index::State`, write `.git/index`.
- Implement whole-file unstaging (`unstage_file`):
  - Reset index entry from HEAD commit tree.
- Implement hunk-level staging (`stage_hunk`) and unstaging (`unstage_hunk`):
  - Apply hunk patch lines in memory to the index blob, write modified blob to ODB, update index entry.
  - Zero disk tempfiles: no `tempfile::Builder` or `git apply --cached`.

### Step 5.3: Native Discard Action
- **Location**: `crates/diffuse-git/src/mutation.rs`
- Implement file and hunk discard:
  - Capture safety stash checkpoint.
  - For whole-file discard: write file content from index/HEAD directly to working tree path.
  - For hunk discard: apply inverse hunk in memory and write updated content to working tree path.
- Test criteria: Integration tests in `crates/diffuse-git/src/mutation/tests.rs` verifying stage, unstage, discard, and safety stash in test repositories with assertions on file content and index state.

---

## 6. Phase 6 — Comparative Stress Profiling Suite (`gix` vs `git`) & Subprocess Evaluation

Conduct heavy, targeted profiling benchmarks comparing pure `gix` directly against `git` CLI subprocesses to determine whether subprocess optimizations are justified.

### Step 6.1: Dedicated Performance Benchmark Suite
- **Location**: `crates/diffuse-git/benches/gix_vs_git.rs`
- Implement realistic stress benchmarks exercising both implementations across four distinct operational dimensions:
  1. **High-Volume Batch Blob Extraction**: Fetch 1,000+ blobs across 200 commits in a real large codebase (e.g. `diffuse` repository history or synthetic multi-commit repo). Compare `gix` multi-threaded in-process reading against `git cat-file --batch`.
  2. **Deep Worktree & Dirwalk Scanning**: Scan a deep directory hierarchy containing 20,000+ files with thousands of ignored artifacts (`node_modules/`, `target/`, nested `.gitignore` rules). Compare `gix::dir::walk` against `git ls-files --others --exclude-standard`.
  3. **Large Changeset Diff Generation with Renames**: Generate full unified diff for changesets with 200+ modified files, large additions/deletions, renames (`-M`), and whitespace suppression (`-w`). Compare in-process `gix-diff` against `git diff -U4 -M -w`.
  4. **Rapid-Fire Staging & Mutation**: Benchmark sequential staging and unstaging of 50 individual hunks. Compare direct in-memory `gix::index::State` updates against `git apply --cached` with temporary files.

### Step 6.2: Empirical Decision Gate & Subprocess Optimization Policy
- Analyze benchmark measurements:
  - If `gix` is roughly on par or faster across all dimensions (which is the default expectation due to eliminating process fork/exec, pipe serialization, and OS context switching):
    - **Maintain `diffuse-git` as 100% pure Rust**. Do not build, maintain, or ship external git subprocess wrappers.
  - If, and only if, a specific operation shows git CLI to be radically faster (e.g. >2x faster on massive worktree scans):
    - Introduce an isolated, optional subprocess acceleration path strictly for that specific operation, protected by a compatibility check and fallback invariant.

### Step 6.3: Verification Gate & Golden Regression Baseline
- Run `./scripts/check.sh` ensuring all format checks, clippy lints, unit tests, and 35 golden tests pass with zero warnings.
- Run test suite with `PATH=""` to prove total autonomy from external git binaries.