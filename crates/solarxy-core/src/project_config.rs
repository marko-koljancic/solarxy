//! Project-level configuration loaded from `solarxy.toml`.
//!
//! Studios set per-project validation policy here: per-category triangle
//! budgets, regex rules that map a model's path to an [`AssetCategory`], and
//! toggles + thresholds for individual validation checks.
//!
//! [`discover`] resolves a config file in priority order — explicit CLI path,
//! `$SOLARXY_CONFIG` env var, the start directory, then walking up to a
//! containing git root. When none is found the caller falls back to
//! [`ProjectConfig::default`].

use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::validation::config::{ValidationConfig, ValidationThresholds};

/// `format_version` value written by this build.
pub const FORMAT_VERSION_CURRENT: u32 = 1;

/// Cap for the parent-walk discovery step (keeps the search bounded on giant
/// mono-repos even when no git root is found).
const WALK_UP_MAX_LEVELS: usize = 20;

/// Filename of the project config when discovered from disk.
pub const CONFIG_FILE_NAME: &str = "solarxy.toml";

/// Environment variable taking precedence over directory-based discovery.
pub const CONFIG_ENV_VAR: &str = "SOLARXY_CONFIG";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct ProjectConfig {
    #[serde(default = "default_format_version")]
    pub format_version: u32,
    #[serde(default)]
    pub budgets: Budgets,
    #[serde(default)]
    pub validation: ValidationConfig,
    #[serde(default)]
    pub thresholds: ValidationThresholds,
    #[serde(default)]
    pub filenames: FilenameClassifier,
    #[serde(default)]
    pub review: ReviewSettings,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            format_version: FORMAT_VERSION_CURRENT,
            budgets: Budgets::default(),
            validation: ValidationConfig::default(),
            thresholds: ValidationThresholds::default(),
            filenames: FilenameClassifier::default(),
            review: ReviewSettings::default(),
        }
    }
}

fn default_format_version() -> u32 {
    FORMAT_VERSION_CURRENT
}

/// Generates the [`ProjectConfig`] JSON Schema as a pretty-printed string.
///
/// `schemars` annotates numeric fields with non-standard `format` values
/// (`uint32`, `float`, and similar) that strict JSON Schema validators
/// reject as unknown formats. Those annotations carry no validation meaning
/// — `type` plus `minimum` already constrain the values — so they are
/// removed here. Shared by the `gen_schemas` example and the `schema_drift`
/// test so both emit identical output.
///
/// # Errors
///
/// Returns the underlying `serde_json` error if the generated schema fails
/// to round-trip through a [`serde_json::Value`] (not expected in practice).
#[cfg(feature = "schemars-gen")]
pub fn schema_json() -> Result<String, serde_json::Error> {
    let schema = schemars::schema_for!(ProjectConfig);
    let mut value = serde_json::to_value(&schema)?;
    strip_nonstandard_formats(&mut value);
    serde_json::to_string_pretty(&value)
}

/// Recursively removes the non-standard numeric `format` annotations
/// `schemars` emits for Rust integer and float types — see [`schema_json`].
#[cfg(feature = "schemars-gen")]
fn strip_nonstandard_formats(value: &mut serde_json::Value) {
    const NUMERIC_FORMATS: &[&str] = &[
        "int8", "int16", "int32", "int64", "int128", "isize", "int", "uint8", "uint16", "uint32",
        "uint64", "uint128", "usize", "uint", "float", "double",
    ];
    match value {
        serde_json::Value::Object(map) => {
            let drop_format = matches!(
                map.get("format"),
                Some(serde_json::Value::String(f)) if NUMERIC_FORMATS.contains(&f.as_str())
            );
            if drop_format {
                map.remove("format");
            }
            for child in map.values_mut() {
                strip_nonstandard_formats(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                strip_nonstandard_formats(item);
            }
        }
        _ => {}
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub enum AssetCategory {
    Hero,
    Prop,
    Environment,
    Default,
}

/// The category as a report reads it. `Default` is a real answer, not a
/// missing one: it means the filename matched no rule, which is why the
/// analyzer distinguishes it from having classified nothing at all.
impl std::fmt::Display for AssetCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AssetCategory::Hero => "Hero",
            AssetCategory::Prop => "Prop",
            AssetCategory::Environment => "Environment",
            AssetCategory::Default => "Default",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct Budgets {
    #[serde(default = "Budgets::default_hero")]
    pub hero: u32,
    #[serde(default = "Budgets::default_prop")]
    pub prop: u32,
    #[serde(default = "Budgets::default_environment")]
    pub environment: u32,
    #[serde(default = "Budgets::default_default")]
    pub default: u32,
}

impl Budgets {
    pub fn for_category(&self, category: AssetCategory) -> u32 {
        match category {
            AssetCategory::Hero => self.hero,
            AssetCategory::Prop => self.prop,
            AssetCategory::Environment => self.environment,
            AssetCategory::Default => self.default,
        }
    }

    const fn default_hero() -> u32 {
        100_000
    }
    const fn default_prop() -> u32 {
        20_000
    }
    const fn default_environment() -> u32 {
        50_000
    }
    const fn default_default() -> u32 {
        30_000
    }
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            hero: Self::default_hero(),
            prop: Self::default_prop(),
            environment: Self::default_environment(),
            default: Self::default_default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct FilenameClassifier {
    #[serde(default)]
    pub rules: Vec<ClassifierRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct ClassifierRule {
    pub pattern: String,
    pub category: AssetCategory,
}

impl FilenameClassifier {
    /// Classifies `path` against this classifier's rules in order. First
    /// matching regex wins. Recompiles regex set on every call — for hot
    /// paths, use [`FilenameClassifier::compile_rules`] + [`classify_compiled`].
    pub fn classify(&self, path: &Path) -> AssetCategory {
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            return AssetCategory::Default;
        };
        for rule in &self.rules {
            if let Ok(re) = Regex::new(&rule.pattern)
                && re.is_match(name)
            {
                return rule.category;
            }
        }
        AssetCategory::Default
    }

    /// Compiles all patterns once; returns an error if any pattern is invalid.
    /// Pair with [`classify_compiled`] for batch classification.
    pub fn compile_rules(&self) -> Result<Vec<(Regex, AssetCategory)>, ProjectConfigError> {
        self.rules
            .iter()
            .enumerate()
            .map(|(idx, rule)| match Regex::new(&rule.pattern) {
                Ok(re) => Ok((re, rule.category)),
                Err(e) => Err(ProjectConfigError::Regex {
                    path: PathBuf::new(),
                    rule_index: idx,
                    pattern: rule.pattern.clone(),
                    source: e,
                }),
            })
            .collect()
    }
}

/// Project-level review-system settings. User-level prefs (display name
/// for the `author` field, panel open default) live in
/// [`crate::preferences::ReviewPrefs`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct ReviewSettings {
    /// Override location of the `.solarxy-review.json` sidecar. `None`
    /// (default) ⇒ sibling to the model file. Relative paths are resolved
    /// against the model's parent directory (so `".solarxy"` produces
    /// `<model_dir>/.solarxy/<stem>.solarxy-review.json`). Absolute paths
    /// are used as-is.
    #[serde(default)]
    pub sidecar_dir: Option<PathBuf>,
}

/// Classifies `path` against a pre-compiled rule set. First match wins;
/// otherwise [`AssetCategory::Default`].
pub fn classify_compiled(rules: &[(Regex, AssetCategory)], path: &Path) -> AssetCategory {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return AssetCategory::Default;
    };
    for (re, cat) in rules {
        if re.is_match(name) {
            return *cat;
        }
    }
    AssetCategory::Default
}

#[derive(Debug, Error)]
pub enum ProjectConfigError {
    #[error("{path}: I/O error: {source}", path = path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}:{line}:{col}: {message}{hint}", path = path.display())]
    Parse {
        path: PathBuf,
        line: usize,
        col: usize,
        message: String,
        hint: String,
    },
    #[error("{path}: invalid regex in classifier rule {rule_index} ('{pattern}'): {source}", path = path.display())]
    Regex {
        path: PathBuf,
        rule_index: usize,
        pattern: String,
        #[source]
        source: regex::Error,
    },
}

/// Loads and parses a `solarxy.toml` from `path`.
///
/// Emits a `tracing::warn!` if the file's `format_version` doesn't match
/// [`FORMAT_VERSION_CURRENT`] — newer configs are read on a best-effort basis.
pub fn load_file(path: &Path) -> Result<ProjectConfig, ProjectConfigError> {
    let raw = std::fs::read_to_string(path).map_err(|source| ProjectConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let cfg = parse_str(&raw).map_err(|(message, byte_offset)| {
        let (line, col) = byte_offset_to_line_col(&raw, byte_offset);
        let hint = build_did_you_mean_hint(&message);
        ProjectConfigError::Parse {
            path: path.to_path_buf(),
            line,
            col,
            message,
            hint,
        }
    })?;
    if cfg.format_version != FORMAT_VERSION_CURRENT {
        tracing::warn!(
            "{}: format_version = {} (this build supports {}). Reading on best-effort basis.",
            path.display(),
            cfg.format_version,
            FORMAT_VERSION_CURRENT,
        );
    }
    Ok(cfg)
}

fn parse_str(raw: &str) -> Result<ProjectConfig, (String, usize)> {
    toml::from_str::<ProjectConfig>(raw).map_err(|e| {
        let byte = e.span().map_or(0, |r| r.start);
        (e.message().to_string(), byte)
    })
}

/// Converts a byte offset within `src` into a 1-based `(line, column)`.
fn byte_offset_to_line_col(src: &str, byte: usize) -> (usize, usize) {
    let clamped = byte.min(src.len());
    let prefix = &src[..clamped];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
    let col = prefix.rfind('\n').map_or(clamped + 1, |nl| clamped - nl);
    (line, col)
}

/// Parses a toml `unknown field` message and appends a Jaro-Winkler-derived
/// suggestion. Returns an empty string when no useful hint can be derived.
fn build_did_you_mean_hint(message: &str) -> String {
    let Some(unknown_start) = message.find("unknown field `") else {
        return String::new();
    };
    let after = &message[unknown_start + "unknown field `".len()..];
    let Some(end) = after.find('`') else {
        return String::new();
    };
    let unknown = &after[..end];
    let expected = extract_backticked_candidates(&after[end + 1..]);
    if expected.is_empty() {
        return String::new();
    }
    let mut best: Option<(f64, &str)> = None;
    for candidate in &expected {
        let score = strsim::jaro_winkler(unknown, candidate);
        if score >= 0.75 && best.is_none_or(|(b, _)| score > b) {
            best = Some((score, candidate));
        }
    }
    best.map_or(String::new(), |(_, name)| {
        format!(" (did you mean '{name}'?)")
    })
}

fn extract_backticked_candidates(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find('`') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('`') else { break };
        out.push(&rest[..end]);
        rest = &rest[end + 1..];
    }
    out
}

/// Resolves a `solarxy.toml` according to:
///
/// 1. `explicit` path (errors if missing).
/// 2. `$SOLARXY_CONFIG` env var (errors if missing).
/// 3. `start/solarxy.toml`.
/// 4. Walk parents until one contains `.git/` or the filesystem root, capped
///    at 20 levels deep (perf safety on huge mono-repos).
///
/// Returns `Ok(None)` when no config is found — caller falls back to
/// [`ProjectConfig::default`].
pub fn discover(
    start: &Path,
    explicit: Option<&Path>,
) -> Result<Option<(PathBuf, ProjectConfig)>, ProjectConfigError> {
    if let Some(p) = explicit {
        let cfg = load_file(p)?;
        return Ok(Some((p.to_path_buf(), cfg)));
    }
    if let Ok(env_path) = std::env::var(CONFIG_ENV_VAR) {
        let p = PathBuf::from(env_path);
        let cfg = load_file(&p)?;
        return Ok(Some((p, cfg)));
    }
    let local = start.join(CONFIG_FILE_NAME);
    if local.is_file() {
        let cfg = load_file(&local)?;
        return Ok(Some((local, cfg)));
    }
    let mut current = start;
    for _ in 0..WALK_UP_MAX_LEVELS {
        let candidate = current.join(CONFIG_FILE_NAME);
        if candidate.is_file() {
            let cfg = load_file(&candidate)?;
            return Ok(Some((candidate, cfg)));
        }
        if current.join(".git").exists() {
            return Ok(None);
        }
        let Some(parent) = current.parent() else {
            return Ok(None);
        };
        current = parent;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tempdir() -> PathBuf {
        // A per-process atomic counter — a wall-clock suffix can collide
        // between tests that run in the same nanosecond on parallel
        // threads, leaking one test's `solarxy.toml` into another's dir.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!(
            "solarxy-project-config-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&base).expect("tempdir");
        base
    }

    #[test]
    fn parser_round_trip_default() {
        let cfg = ProjectConfig::default();
        let s = toml::to_string(&cfg).expect("serialize");
        let parsed: ProjectConfig = toml::from_str(&s).expect("deserialize");
        assert_eq!(parsed.format_version, cfg.format_version);
        assert_eq!(parsed.budgets.hero, cfg.budgets.hero);
        assert!(
            (parsed.thresholds.triangle_budget_tolerance_percent
                - cfg.thresholds.triangle_budget_tolerance_percent)
                .abs()
                < f32::EPSILON
        );
        assert_eq!(parsed.filenames.rules.len(), cfg.filenames.rules.len());
    }

    #[test]
    fn missing_optional_fields_use_defaults() {
        let toml_src = r"
            [budgets]
            hero = 200000
        ";
        let cfg: ProjectConfig = toml::from_str(toml_src).expect("parse");
        assert_eq!(cfg.budgets.hero, 200_000);
        assert_eq!(cfg.budgets.prop, Budgets::default_prop());
        assert_eq!(cfg.format_version, FORMAT_VERSION_CURRENT);
        assert!((cfg.thresholds.flipped_normal_dot - (-0.5)).abs() < f32::EPSILON);
    }

    #[test]
    fn unknown_field_yields_did_you_mean() {
        let dir = tempdir();
        let path = dir.join(CONFIG_FILE_NAME);
        fs::write(&path, "[thresholds]\nflipped_normal_dott = -0.5\n").expect("write");
        let err = load_file(&path).expect_err("must error");
        let msg = err.to_string();
        assert!(
            msg.contains("did you mean 'flipped_normal_dot'?"),
            "got: {msg}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn format_version_mismatch_is_warning_not_error() {
        let dir = tempdir();
        let path = dir.join(CONFIG_FILE_NAME);
        fs::write(&path, "format_version = 99\n").expect("write");
        let cfg = load_file(&path).expect("loads despite mismatch");
        assert_eq!(cfg.format_version, 99);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn review_settings_default_has_no_sidecar_override() {
        let r = ReviewSettings::default();
        assert!(r.sidecar_dir.is_none());
    }

    #[test]
    fn review_settings_missing_section_uses_defaults() {
        let toml_src = r"
            [budgets]
            hero = 50000
        ";
        let cfg: ProjectConfig = toml::from_str(toml_src).expect("parses without [review]");
        assert!(cfg.review.sidecar_dir.is_none());
        assert_eq!(cfg.budgets.hero, 50_000);
    }

    #[test]
    fn review_settings_sidecar_dir_roundtrips() {
        let toml_src = r#"
            [review]
            sidecar_dir = ".solarxy"
        "#;
        let cfg: ProjectConfig = toml::from_str(toml_src).expect("parses [review]");
        assert_eq!(
            cfg.review.sidecar_dir.as_deref(),
            Some(Path::new(".solarxy"))
        );
        // Re-serialize and parse again.
        let s = toml::to_string(&cfg).expect("serialize");
        let parsed: ProjectConfig = toml::from_str(&s).expect("re-parse");
        assert_eq!(
            parsed.review.sidecar_dir.as_deref(),
            Some(Path::new(".solarxy"))
        );
    }

    #[test]
    fn review_settings_rejects_unknown_field() {
        let toml_src = r#"
            [review]
            sidecar_dir = ".x"
            unknown_field = true
        "#;
        let err = toml::from_str::<ProjectConfig>(toml_src).expect_err("unknown rejected");
        let msg = err.to_string();
        assert!(msg.contains("unknown_field"), "got: {msg}");
    }

    #[test]
    fn classify_matches_first_pattern() {
        let cls = FilenameClassifier {
            rules: vec![
                ClassifierRule {
                    pattern: "^hero_".into(),
                    category: AssetCategory::Hero,
                },
                ClassifierRule {
                    pattern: "^env_".into(),
                    category: AssetCategory::Environment,
                },
            ],
        };
        assert_eq!(
            cls.classify(Path::new("/x/hero_sword.glb")),
            AssetCategory::Hero
        );
        assert_eq!(
            cls.classify(Path::new("/x/env_rock.glb")),
            AssetCategory::Environment
        );
        assert_eq!(
            cls.classify(Path::new("/x/random.glb")),
            AssetCategory::Default
        );
        let compiled = cls.compile_rules().expect("compile");
        assert_eq!(
            classify_compiled(&compiled, Path::new("/x/hero_sword.glb")),
            AssetCategory::Hero
        );
    }

    #[test]
    fn discover_prefers_explicit() {
        let dir = tempdir();
        let custom = dir.join("custom.toml");
        let in_dir = dir.join(CONFIG_FILE_NAME);
        fs::write(&custom, "[budgets]\nhero = 999\n").expect("write");
        fs::write(&in_dir, "[budgets]\nhero = 111\n").expect("write");
        let found = discover(&dir, Some(&custom))
            .expect("discover")
            .expect("some");
        assert_eq!(found.0, custom);
        assert_eq!(found.1.budgets.hero, 999);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_walks_up_to_git_root() {
        let root = tempdir();
        fs::create_dir_all(root.join(".git")).expect("git dir");
        fs::write(root.join(CONFIG_FILE_NAME), "[budgets]\nhero = 777\n").expect("config");
        let nested = root.join("a").join("b").join("c");
        fs::create_dir_all(&nested).expect("nested");
        let found = discover(&nested, None).expect("discover").expect("some");
        assert_eq!(found.0, root.join(CONFIG_FILE_NAME));
        assert_eq!(found.1.budgets.hero, 777);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn discover_returns_none_when_absent() {
        let root = tempdir();
        fs::create_dir_all(root.join(".git")).expect("git dir");
        let found = discover(&root, None).expect("discover");
        assert!(found.is_none(), "expected None, got {found:?}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn byte_offset_to_line_col_basic() {
        let src = "abc\ndef\nghi";
        assert_eq!(byte_offset_to_line_col(src, 0), (1, 1));
        assert_eq!(byte_offset_to_line_col(src, 2), (1, 3));
        assert_eq!(byte_offset_to_line_col(src, 4), (2, 1));
        assert_eq!(byte_offset_to_line_col(src, 8), (3, 1));
    }
}
