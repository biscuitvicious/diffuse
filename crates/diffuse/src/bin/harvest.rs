//! harvest: Automated fixture generator and dataset extractor.
//!
//! Harvests multi-file git diffs with old and new side snapshots from any
//! repository into a golden corpus directory (`diff.patch`, `files/`,
//! `meta.json`) conforming to tests/AGENTS.md. The recorded revision is
//! pinned to absolute full commit ids, so a golden can be regenerated
//! byte-for-byte by re-running the engine over the frozen inputs.

use std::{
    collections::HashMap,
    env,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process,
};

use clap::Parser;
use serde::Serialize;

use diffuse::{git::load_git_repo, prelude::*};
use diffuse_git::Repository;

#[derive(Parser, Debug)]
#[command(
    name = "harvest",
    version,
    about = "Harvest git diffs into golden fixtures conforming to tests/AGENTS.md",
    long_about = "harvest captures multi-file git diffs with old- and new-side snapshots from any \
git repository into frozen golden corpus directories conforming to tests/AGENTS.md.\n\n\
The recorded revision is resolved and pinned to absolute full commit IDs, ensuring \
deterministic, byte-identical acceptance testing across engine changes.",
    after_help = "TYPICAL USAGE & EXAMPLES:
    harvest HEAD~1..HEAD -o tests/golden/my-fixture
    harvest WORKTREE -o tests/golden/my-worktree
    harvest --exclude tests/golden -o tests/golden/my-fixture main..feature"
)]
struct Args {
    /// Git revision or range (e.g. HEAD~1..HEAD, main..feature, HEAD).
    /// Use the literal `WORKTREE` to diff the working tree against HEAD
    /// (for uncommitted-change fixtures).
    #[arg(
        value_name = "REVISION",
        default_value = "HEAD~1..HEAD",
        help = "Git revision or range (e.g. HEAD~1..HEAD, main..feature, WORKTREE)"
    )]
    revision: String,

    /// Change to DIR before doing anything (equivalent to git -C <DIR>).
    #[arg(
        short = 'C',
        value_name = "DIR",
        value_hint = clap::ValueHint::DirPath,
        action = clap::ArgAction::Append,
        help_heading = "Repository & Input",
        help = "Change to DIR before doing anything (equivalent to git -C <DIR>)"
    )]
    directory: Vec<PathBuf>,

    /// Target repository path (defaults to current directory)
    #[arg(
        short,
        long,
        value_name = "PATH",
        value_hint = clap::ValueHint::DirPath,
        help_heading = "Repository & Input",
        help = "Target repository path (defaults to current directory)"
    )]
    repo: Option<PathBuf>,

    /// Output fixture path (defaults to stdout)
    #[arg(
        short,
        long,
        value_name = "PATH",
        value_hint = clap::ValueHint::DirPath,
        help_heading = "Output",
        help = "Output fixture directory path (e.g. tests/golden/<name>)"
    )]
    output: Option<PathBuf>,

    /// Lines of context around changes
    #[arg(
        short,
        long,
        value_name = "LINES",
        default_value_t = DEFAULT_CONTEXT_LINES,
        help_heading = "Diff Options",
        help = "Lines of context around changes"
    )]
    context: usize,

    /// Glob pathspecs to exclude from the harvested diff (git pathspec
    /// syntax, e.g. `tests/golden` — passed as `:(exclude)<pattern>` so a
    /// corpus never diffs the golden tree itself).
    #[arg(
        long,
        value_name = "PATTERN",
        help_heading = "Diff Options",
        help = "Pathspec patterns to exclude from harvested diff"
    )]
    exclude: Vec<String>,
}

/// Maps exclusion patterns to git exclude-pathspecs.
fn exclude_pathspecs(patterns: &[String]) -> Vec<String> {
    patterns
        .iter()
        .map(|pattern| format!(":(exclude){pattern}"))
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FixtureOutput {
    #[serde(skip_serializing)]
    diff: String,
    #[serde(skip_serializing)]
    new_content: HashMap<String, String>,
    #[serde(skip_serializing)]
    old_content: HashMap<String, String>,
    /// The revision (range) this fixture was harvested from, pinned to
    /// absolute full commit ids (`WORKTREE` records the working tree's
    /// `HEAD`).  The diff and the old/new side contents travel as separate
    /// corpus files (`diff.patch`, `files/*.old.*`, `files/*.new.*`) — never
    /// inside the JSON.
    revision: String,
}

/// Resolves a git revision or range expression to absolute full commit ids so
/// the emitted fixture pins the exact harvested history — a recorded
/// `HEAD~150..HEAD` would silently re-resolve to a different window when the
/// same string is used again later.
fn resolve_revision(repo_root: &Path, revision: &str) -> Result<String, String> {
    if revision == "WORKTREE" {
        return Ok(revision.to_string());
    }
    let repo = Repository::open(repo_root).map_err(|e| format!("cannot open repository: {e}"))?;
    let resolve_side = |side: &str| -> Result<String, String> {
        let side = if side.is_empty() { "HEAD" } else { side };
        repo.rev_to_full_hash(side)
            .ok_or_else(|| format!("cannot resolve revision '{side}'"))
    };
    let (pos, sep_len) = match (revision.find("..."), revision.find("..")) {
        (Some(i), _) => (i, 3),
        (_, Some(i)) => (i, 2),
        _ => (revision.len(), 0),
    };
    if sep_len == 0 {
        resolve_side(revision)
    } else {
        let left = resolve_side(&revision[..pos])?;
        let right = resolve_side(&revision[pos + sep_len..])?;
        Ok(format!("{left}{}{right}", ".".repeat(sep_len)))
    }
}

/// Stable FNV-1a 64-bit hash — snapshot names must be reproducible across
/// runs, which the std hasher's per-process keys do not guarantee.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Filesystem-safe snapshot file name for a corpus path's old/new side:
/// `<slug>.<side>.<ext>` with the path's `/` folded to `-`.  The side tag
/// and the file's extension always trail the name.  A stem that would exceed
/// the filename budget keeps its TAIL — the file's own name — while the
/// dropped directory prefix is marked with a stable FNV-1a hash of the full
/// path so distinct long paths keep distinct names.
fn snapshot_filename(path: &str, side: &str) -> String {
    const MAX_STEM: usize = 128;
    let slug = path.replace('/', "-");
    let (stem, ext) = match slug.rfind('.') {
        Some(dot_idx) if dot_idx > 0 => (slug[..dot_idx].to_string(), &slug[dot_idx..]),
        _ => (slug, ""),
    };
    if stem.len() <= MAX_STEM {
        return format!("{stem}.{side}{ext}");
    }
    let mut keep_from = stem.len() - (MAX_STEM - 17);
    while !stem.is_char_boundary(keep_from) {
        keep_from += 1;
    }
    format!(
        "{}-{:016x}.{side}{ext}",
        &stem[keep_from..],
        fnv1a(path.as_bytes())
    )
}

fn main() {
    let args = Args::parse();

    for dir in &args.directory {
        if let Err(e) = env::set_current_dir(dir) {
            eprintln!(
                "harvest: cannot change directory to '{}': {e}",
                dir.display()
            );
            process::exit(1);
        }
    }

    let repo_root = args
        .repo
        .unwrap_or_else(|| env::current_dir().expect("valid current dir"));

    // `WORKTREE` diffs the (uncommitted) working tree against HEAD; any
    // other revision expression is pinned to absolute commit ids up front so
    // the diff and the recorded revision are both fixed.
    let worktree = args.revision == "WORKTREE";
    let resolved_revision = if worktree {
        args.revision.clone()
    } else {
        resolve_revision(&repo_root, &args.revision).unwrap_or_else(|e| {
            eprintln!("harvest: {e}");
            process::exit(1);
        })
    };
    let base_args: Vec<String> = if worktree {
        Vec::new()
    } else {
        vec![resolved_revision.clone()]
    };

    let git_res = match load_git_repo(
        &repo_root,
        &base_args,
        &exclude_pathspecs(&args.exclude),
        false,
        args.context,
    ) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("harvest: {e}");
            process::exit(1);
        }
    };

    let revision = if worktree {
        let head = Repository::open(&repo_root)
            .ok()
            .and_then(|r| r.head_commit_hash())
            .unwrap_or_default();
        if head.is_empty() {
            "WORKTREE".to_string()
        } else {
            head
        }
    } else {
        resolved_revision
    };

    let fixture = FixtureOutput {
        diff: git_res.diff,
        new_content: git_res.new_content,
        old_content: git_res.old_content,
        revision,
    };

    if let Some(out_path) = args.output {
        let is_json = out_path.extension().is_some_and(|ext| ext == "json")
            || out_path.to_string_lossy().ends_with(".json.gz");
        if is_json {
            let json = serde_json::to_string_pretty(&fixture).expect("serialize fixture");
            if out_path.extension().is_some_and(|ext| ext == "gz") {
                let file = File::create(&out_path).expect("create output file");
                let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::best());
                encoder.write_all(json.as_bytes()).expect("write gzip data");
                encoder.finish().expect("finish gzip compression");
            } else {
                fs::write(&out_path, json).expect("write output file");
            }
            eprintln!(
                "Harvested {} files to {}",
                fixture.new_content.len(),
                out_path.display()
            );
        } else {
            // Directory structure
            let corpus_dir = out_path;
            let files_dir = corpus_dir.join("files");
            fs::create_dir_all(&files_dir).expect("create files dir");

            // Write diff.patch
            fs::write(corpus_dir.join("diff.patch"), &fixture.diff).expect("write diff.patch");

            let mut all_paths: Vec<String> = fixture
                .new_content
                .keys()
                .chain(fixture.old_content.keys())
                .cloned()
                .collect();
            all_paths.sort();
            all_paths.dedup();

            #[derive(Serialize)]
            struct MetaRecord {
                path: String,
                #[serde(skip_serializing_if = "Option::is_none")]
                old: Option<String>,
                #[serde(skip_serializing_if = "Option::is_none")]
                new: Option<String>,
            }

            #[derive(Serialize)]
            struct MetaOutput {
                revision: String,
                files: Vec<MetaRecord>,
            }

            let mut meta_records = Vec::new();
            for p in all_paths {
                let mut rec = MetaRecord {
                    path: p.clone(),
                    old: None,
                    new: None,
                };
                if let Some(old_str) = fixture.old_content.get(&p) {
                    let old_name = snapshot_filename(&p, "old");
                    fs::write(files_dir.join(&old_name), old_str).expect("write old file");
                    rec.old = Some(old_name);
                }
                if let Some(new_str) = fixture.new_content.get(&p) {
                    let new_name = snapshot_filename(&p, "new");
                    fs::write(files_dir.join(&new_name), new_str).expect("write new file");
                    rec.new = Some(new_name);
                }
                meta_records.push(rec);
            }

            let meta = MetaOutput {
                revision: fixture.revision,
                files: meta_records,
            };
            let meta_json = serde_json::to_string_pretty(&meta).expect("serialize meta.json");
            fs::write(corpus_dir.join("meta.json"), meta_json).expect("write meta.json");

            eprintln!(
                "Harvested {} files to {}",
                fixture.new_content.len(),
                corpus_dir.display()
            );
        }
    } else {
        let json = serde_json::to_string_pretty(&fixture).expect("serialize fixture");
        println!("{json}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diffuse_git::testing::{commit_files, init_repo};

    #[test]
    fn resolves_revision_ranges_to_fixed_commit_ids() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path()).expect("init repo");
        let base = commit_files(&repo, "one", &[("a.txt", "one")])
            .expect("commit 1")
            .to_string();
        let head = commit_files(&repo, "two", &[("a.txt", "two")])
            .expect("commit 2")
            .to_string();

        assert_eq!(
            resolve_revision(dir.path(), "HEAD~1..HEAD").expect("resolve range"),
            format!("{base}..{head}"),
            "relative range pins to absolute commit ids"
        );
        assert_eq!(
            resolve_revision(dir.path(), "HEAD").expect("resolve single"),
            head,
            "single revision resolves to its full commit id"
        );
        assert_eq!(
            resolve_revision(dir.path(), "HEAD~1...HEAD").expect("resolve merge-base range"),
            format!("{base}...{head}"),
            "three-dot separator is preserved"
        );
        assert_eq!(
            resolve_revision(dir.path(), "..HEAD").expect("resolve open-left range"),
            format!("{head}..{head}"),
            "empty range side means HEAD"
        );
        assert_eq!(
            resolve_revision(dir.path(), "WORKTREE").expect("worktree passthrough"),
            "WORKTREE",
            "WORKTREE is a special mode, not a revision"
        );
        assert!(
            resolve_revision(dir.path(), "no-such-rev").is_err(),
            "unresolvable revisions fail loudly"
        );
    }

    #[test]
    fn exclude_patterns_map_to_git_exclude_pathspecs() {
        assert!(exclude_pathspecs(&[]).is_empty(), "no patterns, no specs");
        assert_eq!(
            exclude_pathspecs(&["tests/golden".to_string(), "dist".to_string()]),
            vec![":(exclude)tests/golden", ":(exclude)dist"],
            "each pattern becomes a git exclude pathspec"
        );
    }

    #[test]
    fn snapshot_names_slug_paths_and_cap_length() {
        assert_eq!(
            snapshot_filename("src/collapse/mod.rs", "old"),
            "src-collapse-mod.old.rs",
            "path folds to a side-tagged snapshot name"
        );
        assert_eq!(
            snapshot_filename("Makefile", "new"),
            "Makefile.new",
            "extension-less paths get a plain side suffix"
        );

        // Overflow keeps the TAIL of the slug — the file's own name — with a
        // stable hash of the full path in front; side tag and extension still
        // trail.
        let deep = "very-long-directory-name/".repeat(12).to_string() + "mod.rs";
        let deep_name = snapshot_filename(&deep, "old");
        assert!(deep_name.len() <= 255, "fits the filesystem limit");
        assert!(deep_name.ends_with(".old.rs"), "side and extension survive");
        let core = deep_name
            .strip_suffix(".old.rs")
            .expect("side and extension");
        let (tail, hash) = core.rsplit_once('-').expect("hash separator");
        assert_eq!(
            hash.len(),
            16,
            "stable FNV-1a hash marks the dropped prefix"
        );
        assert!(tail.ends_with("mod"), "the file's own name is kept");
        assert_eq!(deep_name, snapshot_filename(&deep, "old"), "stable");

        // A filename longer than the whole budget keeps its own tail.
        let long_a = format!("deep/{}", "x".repeat(200));
        let long_b = format!("deep/{}", "y".repeat(200));
        for (name, fill) in [
            (snapshot_filename(&long_a, "old"), 'x'),
            (snapshot_filename(&long_b, "old"), 'y'),
        ] {
            assert!(name.len() <= 255, "fits the filesystem limit");
            assert!(name.ends_with(".old"), "side tag survives");
            let core = name.strip_suffix(".old").expect("side tag");
            let (tail, hash) = core.rsplit_once('-').expect("hash separator");
            assert_eq!(hash.len(), 16, "stable FNV-1a hash suffix");
            assert!(
                tail.chars().all(|c| c == fill),
                "the filename's tail survives: {tail}"
            );
        }
        assert_ne!(
            snapshot_filename(&long_a, "old"),
            snapshot_filename(&long_b, "old"),
            "distinct long paths hash to distinct names"
        );

        // A long path with a compound extension keeps the extension.
        let tgz = format!("deep/{}.tar.gz", "z".repeat(200));
        assert!(
            snapshot_filename(&tgz, "new").ends_with(".new.gz"),
            "compound extension survives truncation"
        );
    }

    #[test]
    fn test_harvest_clap_debug_assert_and_help() {
        use clap::CommandFactory;
        Args::command().debug_assert();
        let mut cmd = Args::command();
        let help = cmd.render_help().to_string();
        assert!(help.contains("Repository & Input:"));
        assert!(help.contains("Output:"));
        assert!(help.contains("Diff Options:"));
        assert!(help.contains("TYPICAL USAGE & EXAMPLES:"));
    }
}
