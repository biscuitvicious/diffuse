//! Extension to language profile mapping table and fence tag normalization.

use std::sync::LazyLock;

use crate::{
    language::{c_family, config, go, markdown, python, rust, shell, web},
    profile::LangProfile,
};

/// Normalize a fence tag (or file extension) to a profile key.
///
/// Language aliases map to their profile (`rust`→`rs`, `typescript`→`ts`…).
/// A Rust doc-comment fence carries a comma-separated **doctest attribute
/// list** — `rust,no_run`, `should_panic`, `edition2021`, `ignore_linux`, or
/// combos like `rust,no_run,edition2021`. Any `rust` token ⇒ a Rust example;
/// any modifier (`no_run`, `ignore`, `should_panic`, `compile_fail`,
/// `edition*`, `ignore_*`) ⇒ Rust is implied (the default lang in a `.rs`
/// doc fence is rust). A named language alias embedded in a rust crate's
/// doc (` ```python `) takes precedence and highlights as that language.
pub fn fence_lang_ext(tag: &str) -> &str {
    let mut rust_implied = false;
    for part in tag.split(',') {
        let p = part.trim();
        match p {
            "rust" => return "rs",
            "javascript" | "jsx" => return "js",
            "typescript" | "tsx" => return "ts",
            "python" => return "py",
            "shell" | "bash" => return "sh",
            "c++" | "cpp" => return "cpp",
            "markdown" | "md" => return "md",
            "json" | "jsonl" => return "json",
            "ignore" | "no_run" | "should_panic" | "compile_fail" => rust_implied = true,
            "text" | "txt" | "plaintext" | "console" | "log" => return "",
            _ if p.starts_with("edition") || p.starts_with("ignore_") => rust_implied = true,
            _ => {}
        }
    }
    if rust_implied { "rs" } else { tag }
}

/// All registered file extensions supported by the registry.
pub const REGISTERED_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "py", "sh", "bash", "c", "h", "cpp", "hpp", "java",
    "go", "md", "markdown", "toml", "yaml", "yml", "json", "jsonl",
];

/// Returns all distinct language profiles registered in the syntax system.
pub fn all_registered_profiles() -> [&'static LangProfile; 10] {
    static RS: LazyLock<LangProfile> = LazyLock::new(rust::profile);
    static WEB: LazyLock<LangProfile> = LazyLock::new(web::profile);
    static PY: LazyLock<LangProfile> = LazyLock::new(python::profile);
    static SH: LazyLock<LangProfile> = LazyLock::new(shell::profile);
    static C_FAM: LazyLock<LangProfile> = LazyLock::new(c_family::profile);
    static GO: LazyLock<LangProfile> = LazyLock::new(go::profile);
    static MD: LazyLock<LangProfile> = LazyLock::new(markdown::profile);
    static TOML: LazyLock<LangProfile> = LazyLock::new(config::toml_profile);
    static YAML: LazyLock<LangProfile> = LazyLock::new(config::yaml_profile);
    static JSON: LazyLock<LangProfile> = LazyLock::new(config::json_profile);

    [
        &*RS, &*WEB, &*PY, &*SH, &*C_FAM, &*GO, &*MD, &*TOML, &*YAML, &*JSON,
    ]
}

pub fn lang_profile(ext: &str) -> Option<&'static LangProfile> {
    let [rs, web, py, sh, c_fam, go, md, toml, yaml, json] = all_registered_profiles();

    // Fence ids are LANGUAGE names (` ```rust `); the profiles are keyed by
    // the file EXTENSIONS (`rs`). Map the common aliases — and the rust
    // doctest attribute lists — so a labeled fence actually highlights.
    let ext = fence_lang_ext(ext);
    match ext {
        "rs" => Some(rs),
        "ts" | "tsx" | "js" | "jsx" | "mjs" => Some(web),
        "py" => Some(py),
        "sh" | "bash" => Some(sh),
        "c" | "h" | "cpp" | "hpp" | "java" => Some(c_fam),
        "go" => Some(go),
        "md" | "markdown" => Some(md),
        "toml" => Some(toml),
        "yaml" | "yml" => Some(yaml),
        "json" | "jsonl" => Some(json),
        _ => None,
    }
}
