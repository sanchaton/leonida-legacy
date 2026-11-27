//! Graphics profiles and user overrides, resolved before the GPU reads its switches.
//!
//! Resolution is pure: callers provide the adapter and environment, then apply the
//! returned plan at startup. This module never changes the process environment.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

const CATALOG: &str = include_str!("../../../gpu-profiles.json");

pub type Environment = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Adapter {
    pub name: String,
    pub vendor: String,
    pub vram_mb: u64,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphicsSettings {
    /// Omitted: match the adapter. Empty or containing `none`: apply no profiles.
    pub profiles: Option<Vec<String>>,
    pub async_submit: Option<bool>,
    pub async_present: Option<bool>,
    pub queue2: Option<u8>,
    pub indirect_window: Option<bool>,
    pub copy_plain: Option<bool>,
    pub join_parts: Option<bool>,
    pub back_own_view: Option<bool>,
    pub indirect_gather: Option<bool>,
    pub card_rehash_runs: Option<bool>,
    pub window_upload: Option<bool>,
    pub card_windows_mb: Option<u64>,
    pub texture_room_percent: Option<u8>,
    pub texture_stage_mb: Option<u64>,
}

impl GraphicsSettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.queue2.is_none_or(|n| n <= 3),
            "[graphics].queue2 must be between 0 and 3"
        );
        ensure!(
            self.card_windows_mb.is_none_or(|n| n <= u64::MAX >> 20),
            "[graphics].card_windows_mb is too large to convert to bytes"
        );
        ensure!(
            self.texture_room_percent
                .is_none_or(|n| (50..=97).contains(&n)),
            "[graphics].texture_room_percent must be between 50 and 97"
        );
        ensure!(
            self.texture_stage_mb.is_none_or(|n| (64..=4096).contains(&n)),
            "[graphics].texture_stage_mb must be between 64 and 4096"
        );
        if let Some(names) = &self.profiles {
            ensure!(
                names.iter().all(|name| !name.trim().is_empty()),
                "[graphics].profiles contains an empty name; use [] to disable profiles"
            );
        }
        Ok(())
    }

    fn values(&self) -> Environment {
        let mut values = Environment::new();
        for (name, value) in [
            ("ASYNC_SUBMIT", self.async_submit),
            ("ASYNC_PRESENT", self.async_present),
            ("INDIRECT_WINDOW", self.indirect_window),
            ("COPY_PLAIN", self.copy_plain),
            ("JOIN_PARTS", self.join_parts),
            ("BACK_OWN_VIEW", self.back_own_view),
            ("INDIRECT_GATHER", self.indirect_gather),
            ("CARD_REHASH_RUNS", self.card_rehash_runs),
            ("WINDOW_UPLOAD", self.window_upload),
        ] {
            if let Some(value) = value {
                values.insert(format!("LEONIDA_{name}"), u8::from(value).to_string());
            }
        }
        for (name, value) in [
            ("QUEUE2", self.queue2.map(u64::from)),
            ("CARD_WINDOWS_MB", self.card_windows_mb),
            (
                "TEXTURE_ROOM_PERCENT",
                self.texture_room_percent.map(u64::from),
            ),
            ("TEXTURE_STAGE_MB", self.texture_stage_mb),
        ] {
            if let Some(value) = value {
                values.insert(format!("LEONIDA_{name}"), value.to_string());
            }
        }
        values
    }
}

/// Effective switches and their origins. Missing switches retain their code defaults.
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub gpu: Option<Adapter>,
    pub profiles: Vec<String>,
    pub settings: Environment,
    pub sources: BTreeMap<String, String>,
}

impl Plan {
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).context("serializing graphics settings")
    }
}

/// Runtime switches profiles may supply. Paths, selection and early initialization
/// remain under the caller's control; probing an adapter must not change them.
pub fn is_setting_key(name: &str) -> bool {
    name.starts_with("LEONIDA_")
        && name.len() > "LEONIDA_".len()
        && name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && !name.starts_with("LEONIDA_RENDERDOC")
        && !matches!(
            name,
            "LEONIDA_CONFIG"
                | "LEONIDA_GAME_DIR"
                | "LEONIDA_CACHE_DIR"
                | "LEONIDA_DATA_DIR"
                | "LEONIDA_TEST_TITLES"
                | "LEONIDA_LOG"
                | "LEONIDA_PROFILE"
                | "LEONIDA_KEEP_WINDOW"
                | "LEONIDA_GRAPHICS_OFF"
                | "LEONIDA_D3D12_WARP"
                | "LEONIDA_RELAUNCHED_FROM"
                | "LEONIDA_AUDIO_CAPTURE_OF"
        )
}

/// Windows environment keys are case-insensitive. Empty values are explicit too.
/// Refuse non-Unicode switch values instead of replacing them with a profile default.
pub fn snapshot_environment() -> Result<Environment> {
    snapshot_from(std::env::vars_os())
}

fn snapshot_from(
    vars: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> Result<Environment> {
    let mut snapshot = Environment::new();
    for (key, value) in vars {
        let name = key.to_string_lossy().to_ascii_uppercase();
        if !name.starts_with("LEONIDA_") {
            continue;
        }
        ensure!(
            key.to_str().is_some(),
            "a LEONIDA_ variable name is not valid Unicode; unset it"
        );
        if is_setting_key(&name)
            || matches!(name.as_str(), "LEONIDA_PROFILE" | "LEONIDA_GRAPHICS_OFF")
        {
            let value = value.into_string().map_err(|_| {
                anyhow::anyhow!("{name} is not valid Unicode; unset it or set a text value")
            })?;
            snapshot.insert(name, value);
        }
    }
    Ok(snapshot)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    #[serde(default, rename = "about")]
    _about: Vec<String>,
    profiles: Vec<Profile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    name: String,
    #[serde(default, rename = "measured")]
    _measured: Option<String>,
    #[serde(default)]
    opt_in: bool,
    #[serde(default, rename = "match")]
    matcher: Matcher,
    env: BTreeMap<String, Option<String>>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Matcher {
    vendor: Option<String>,
    min_vram_mb: Option<u64>,
    max_vram_mb: Option<u64>,
    name: Option<String>,
}

enum Selection {
    Auto,
    None,
    Named(BTreeSet<String>),
}

fn selection(settings: &GraphicsSettings, env: &Environment) -> Selection {
    let forced = env
        .get("LEONIDA_PROFILE")
        // This spelling already enables the diagnostics CPU sampling profiler.
        // Leave it for that consumer; graphics still use TOML or automatic matching.
        .filter(|value| !value.trim().is_empty() && value.as_str() != "1");
    let names = forced
        .map(|value| value.split(',').map(str::to_owned).collect::<Vec<_>>())
        .or_else(|| settings.profiles.clone());
    match names {
        None => Selection::Auto,
        Some(names) => {
            let names: BTreeSet<_> = names
                .iter()
                .map(|name| name.trim().to_lowercase())
                .filter(|name| !name.is_empty())
                .collect();
            if names.contains("none") || (names.is_empty() && forced.is_none()) {
                Selection::None
            } else if names.is_empty() {
                Selection::Auto
            } else {
                Selection::Named(names)
            }
        }
    }
}

fn normalized(env: &Environment) -> Environment {
    env.iter()
        .map(|(name, value)| (name.to_ascii_uppercase(), value.clone()))
        .collect()
}

/// Reads only the optional local catalog, next to the selected configuration file.
/// `none` and `LEONIDA_GRAPHICS_OFF=1` also work when that catalog is broken.
pub fn load(
    settings: &GraphicsSettings,
    source: Option<&Path>,
    gpu: Option<Adapter>,
    env: &Environment,
) -> Result<Plan> {
    let env = normalized(env);
    let disabled = env
        .get("LEONIDA_GRAPHICS_OFF")
        .is_some_and(|value| value == "1")
        || matches!(selection(settings, &env), Selection::None);
    let local = if disabled {
        None
    } else {
        let parent = source.and_then(Path::parent).unwrap_or(Path::new("."));
        let path = parent.join("gpu-profiles.local.json");
        match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        }
    };
    resolve(settings, gpu, &env, local.as_deref())
}

/// Environment overrides TOML, which overrides profiles in catalog order.
/// A null profile value cancels an earlier profile value, never a user override.
pub fn resolve(
    settings: &GraphicsSettings,
    gpu: Option<Adapter>,
    env: &Environment,
    local_json: Option<&str>,
) -> Result<Plan> {
    settings.validate()?;
    let env = normalized(env);
    let mut plan = Plan {
        gpu,
        profiles: Vec::new(),
        settings: Environment::new(),
        sources: BTreeMap::new(),
    };
    if !env
        .get("LEONIDA_GRAPHICS_OFF")
        .is_some_and(|value| value == "1")
    {
        let selected = selection(settings, &env);
        if !matches!(selected, Selection::None) {
            let mut profiles = parse_catalog(CATALOG, "built-in gpu-profiles.json")?;
            if let Some(local) = local_json {
                profiles.extend(parse_catalog(local, "gpu-profiles.local.json")?);
            }
            if let Selection::Named(names) = &selected {
                for name in names {
                    ensure!(
                        profiles
                            .iter()
                            .any(|profile| profile.name.to_lowercase() == *name),
                        "unknown graphics profile {name:?}; check [graphics].profiles or LEONIDA_PROFILE"
                    );
                }
            }
            for profile in profiles {
                let wanted = match &selected {
                    Selection::Auto => {
                        !profile.opt_in && profile.matcher.matches(plan.gpu.as_ref())?
                    }
                    Selection::Named(names) => {
                        profile.name.eq_ignore_ascii_case("base")
                            || names.contains(&profile.name.to_lowercase())
                    }
                    Selection::None => false,
                };
                if !wanted {
                    continue;
                }
                for (name, value) in profile.env {
                    if let Some(value) = value {
                        plan.settings.insert(name.clone(), value);
                        plan.sources
                            .insert(name, format!("profile:{}", profile.name));
                    } else {
                        plan.settings.remove(&name);
                        plan.sources.remove(&name);
                    }
                }
                plan.profiles.push(profile.name);
            }
        }
        for (name, value) in settings.values() {
            plan.settings.insert(name.clone(), value);
            plan.sources.insert(name, "config".into());
        }
    }
    for (name, value) in env {
        if is_setting_key(&name) {
            ensure!(!value.contains('\0'), "{name} contains a NUL character");
            plan.settings.insert(name.clone(), value);
            plan.sources.insert(name, "environment".into());
        }
    }
    Ok(plan)
}

fn parse_catalog(text: &str, source: &str) -> Result<Vec<Profile>> {
    let catalog: Catalog =
        serde_json::from_str(text).with_context(|| format!("parsing {source}"))?;
    for profile in &catalog.profiles {
        ensure!(
            !profile.name.trim().is_empty() && profile.name.trim() == profile.name,
            "{source}: profile names must be nonempty without surrounding whitespace"
        );
        for (name, value) in &profile.env {
            ensure!(
                is_setting_key(name),
                "{source}: profile {:?} cannot set {name}",
                profile.name
            );
            ensure!(
                value.as_ref().is_none_or(|value| !value.contains('\0')),
                "{source}: {name} contains a NUL character"
            );
        }
        let matcher = &profile.matcher;
        if let Some(vendor) = &matcher.vendor {
            ensure!(
                matches!(
                    vendor.to_ascii_lowercase().as_str(),
                    "nvidia" | "amd" | "intel" | "other"
                ),
                "{source}: profile {:?} has unknown vendor {vendor:?}",
                profile.name
            );
        }
        if let (Some(min), Some(max)) = (matcher.min_vram_mb, matcher.max_vram_mb) {
            ensure!(
                min <= max,
                "{source}: profile {:?} has reversed VRAM limits",
                profile.name
            );
        }
        if let Some(pattern) = &matcher.name {
            pattern_tokens(pattern)
                .with_context(|| format!("{source}: profile {:?}", profile.name))?;
        }
    }
    Ok(catalog.profiles)
}

impl Matcher {
    fn matches(&self, gpu: Option<&Adapter>) -> Result<bool> {
        let Some(gpu) = gpu else {
            return Ok(self.vendor.is_none()
                && self.min_vram_mb.is_none()
                && self.max_vram_mb.is_none()
                && self.name.is_none());
        };
        if self
            .vendor
            .as_ref()
            .is_some_and(|vendor| !vendor.eq_ignore_ascii_case(&gpu.vendor))
            || self.min_vram_mb.is_some_and(|min| gpu.vram_mb < min)
            || self.max_vram_mb.is_some_and(|max| gpu.vram_mb > max)
        {
            return Ok(false);
        }
        match &self.name {
            Some(pattern) => wildcard(pattern, &gpu.name),
            None => Ok(true),
        }
    }
}

enum Token {
    Star,
    Any,
    Literal(char),
    Class(Vec<(char, char)>),
}

/// Case-insensitive `*`, `?` and `[abc]` / `[a-z]`; malformed or unsupported
/// escapes are errors rather than silently matching a different adapter.
fn pattern_tokens(pattern: &str) -> Result<Vec<Token>> {
    let chars: Vec<_> = pattern.to_lowercase().chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        tokens.push(match chars[at] {
            '*' => Token::Star,
            '?' => Token::Any,
            '`' => bail!("GPU name patterns do not support backtick escapes"),
            '[' => {
                let start = at + 1;
                let end = chars[start..]
                    .iter()
                    .position(|&c| c == ']')
                    .map(|n| start + n)
                    .context("GPU name pattern has an unclosed '['")?;
                ensure!(start < end, "GPU name pattern has an empty character class");
                ensure!(
                    !matches!(chars[start], '!' | '^'),
                    "GPU name patterns do not support negated character classes"
                );
                let mut ranges = Vec::new();
                at = start;
                while at < end {
                    if at + 2 < end && chars[at + 1] == '-' {
                        ensure!(
                            chars[at] <= chars[at + 2],
                            "GPU name pattern has a reversed range"
                        );
                        ranges.push((chars[at], chars[at + 2]));
                        at += 3;
                    } else {
                        ranges.push((chars[at], chars[at]));
                        at += 1;
                    }
                }
                Token::Class(ranges)
            }
            literal => Token::Literal(literal),
        });
        at += 1;
    }
    Ok(tokens)
}

fn wildcard(pattern: &str, name: &str) -> Result<bool> {
    let name: Vec<_> = name.to_lowercase().chars().collect();
    let mut matched = vec![false; name.len() + 1];
    matched[0] = true;
    for token in pattern_tokens(pattern)? {
        if matches!(token, Token::Star) {
            for i in 1..matched.len() {
                matched[i] |= matched[i - 1];
            }
        } else {
            for i in (1..matched.len()).rev() {
                let fits = match &token {
                    Token::Any => true,
                    Token::Literal(c) => *c == name[i - 1],
                    Token::Class(ranges) => {
                        ranges.iter().any(|&(a, b)| (a..=b).contains(&name[i - 1]))
                    }
                    Token::Star => unreachable!(),
                };
                matched[i] = matched[i - 1] && fits;
            }
            matched[0] = false;
        }
    }
    Ok(matched[name.len()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn env(values: &[(&str, &str)]) -> Environment {
        values
            .iter()
            .map(|&(key, value)| (key.into(), value.into()))
            .collect()
    }

    fn gpu() -> Option<Adapter> {
        Some(Adapter {
            name: "NVIDIA GeForce RTX 5090".into(),
            vendor: "nvidia".into(),
            vram_mb: 32768,
        })
    }

    fn local(profiles: serde_json::Value) -> String {
        json!({ "profiles": profiles }).to_string()
    }

    #[test]
    fn automatic_defaults_are_exactly_the_measured_base() {
        let expected = env(&[
            ("LEONIDA_ASYNC_SUBMIT", "1"),
            ("LEONIDA_QUEUE2", "1"),
            ("LEONIDA_INDIRECT_WINDOW", "1"),
            ("LEONIDA_COPY_PLAIN", "1"),
            ("LEONIDA_JOIN_PARTS", "1"),
            ("LEONIDA_BACK_OWN_VIEW", "1"),
            ("LEONIDA_INDIRECT_GATHER", "1"),
            ("LEONIDA_CARD_REHASH_RUNS", "1"),
        ]);
        for gpu in [gpu(), None] {
            let plan = resolve(&GraphicsSettings::default(), gpu, &env(&[]), None).unwrap();
            assert_eq!(plan.profiles, ["base"]);
            assert_eq!(plan.settings, expected);
            assert!(plan.sources.values().all(|source| source == "profile:base"));
        }
    }

    #[test]
    fn correctness_profiles_are_opt_in_and_respect_environment_overrides() {
        for (profile, key) in [
            ("nvidia-code-tables", "LEONIDA_CODE_TABLE_CHUNKS"),
            ("readback-order", "LEONIDA_READBACK_ORDER"),
        ] {
            let automatic = resolve(&GraphicsSettings::default(), gpu(), &env(&[]), None).unwrap();
            assert!(!automatic.settings.contains_key(key));
            assert!(!automatic.profiles.iter().any(|name| name == profile));

            let settings = GraphicsSettings {
                profiles: Some(vec![profile.into()]),
                ..Default::default()
            };
            let selected = resolve(&settings, gpu(), &env(&[]), None).unwrap();
            assert_eq!(selected.profiles, ["base", profile]);
            assert_eq!(selected.settings[key], "1");
            assert_eq!(selected.sources[key], format!("profile:{profile}"));

            let overridden = resolve(&settings, gpu(), &env(&[(key, "0")]), None).unwrap();
            assert_eq!(overridden.settings[key], "0");
            assert_eq!(overridden.sources[key], "environment");
        }
    }

    #[test]
    fn explicit_profiles_are_case_insensitive_and_include_base() {
        let settings = GraphicsSettings {
            profiles: Some(vec!["missing".into()]),
            ..Default::default()
        };
        let plan = resolve(
            &settings,
            gpu(),
            &env(&[("leonida_profile", " CARD-WINDOWS , ")]),
            None,
        )
        .unwrap();
        assert_eq!(plan.profiles, ["base", "card-windows"]);
        assert_eq!(plan.settings["LEONIDA_CARD_WINDOWS_MB"], "3072");
        assert_eq!(plan.settings["LEONIDA_WINDOW_UPLOAD"], "1");
        assert_eq!(plan.settings["LEONIDA_TEXTURE_ROOM_PERCENT"], "70");
    }

    #[test]
    fn async_present_maps_and_environment_wins() {
        let settings = GraphicsSettings {
            async_present: Some(true),
            ..Default::default()
        };
        let plan = resolve(&settings, gpu(), &env(&[]), None).unwrap();
        assert_eq!(plan.settings["LEONIDA_ASYNC_PRESENT"], "1");
        assert_eq!(plan.sources["LEONIDA_ASYNC_PRESENT"], "config");

        let plan = resolve(
            &settings,
            gpu(),
            &env(&[("LEONIDA_ASYNC_PRESENT", "0")]),
            None,
        )
        .unwrap();
        assert_eq!(plan.settings["LEONIDA_ASYNC_PRESENT"], "0");
        assert_eq!(plan.sources["LEONIDA_ASYNC_PRESENT"], "environment");
    }

    #[test]
    fn texture_stage_mb_is_validated_and_mapped() {
        let settings = GraphicsSettings {
            texture_stage_mb: Some(512),
            ..Default::default()
        };
        let plan = resolve(&settings, gpu(), &env(&[]), None).unwrap();
        assert_eq!(plan.settings["LEONIDA_TEXTURE_STAGE_MB"], "512");
        assert_eq!(plan.sources["LEONIDA_TEXTURE_STAGE_MB"], "config");

        for bad in [0, 32, 5000] {
            let settings = GraphicsSettings {
                texture_stage_mb: Some(bad),
                ..Default::default()
            };
            assert!(
                resolve(&settings, gpu(), &env(&[]), None).is_err(),
                "{bad} must be refused"
            );
        }

        let settings = GraphicsSettings {
            texture_stage_mb: Some(512),
            ..Default::default()
        };
        let plan = resolve(
            &settings,
            gpu(),
            &env(&[("LEONIDA_TEXTURE_STAGE_MB", "256")]),
            None,
        )
        .unwrap();
        assert_eq!(plan.settings["LEONIDA_TEXTURE_STAGE_MB"], "256");
        assert_eq!(plan.sources["LEONIDA_TEXTURE_STAGE_MB"], "environment");
    }

    #[test]
    fn environment_then_toml_then_profile_and_empty_is_explicit() {
        let settings = GraphicsSettings {
            async_submit: Some(false),
            queue2: Some(2),
            copy_plain: Some(false),
            ..Default::default()
        };
        let plan = resolve(
            &settings,
            gpu(),
            &env(&[
                ("leonida_queue2", "0"),
                ("LEONIDA_COPY_PLAIN", ""),
                ("LEONIDA_JOIN_PARTS", "alt"),
                ("LEONIDA_CAPTURE", "C:/測定/capture"),
                ("LEONIDA_CONFIG", "private.toml"),
                ("LEONIDA_D3D12_WARP", "1"),
            ]),
            None,
        )
        .unwrap();
        assert_eq!(plan.settings["LEONIDA_ASYNC_SUBMIT"], "0");
        assert_eq!(plan.sources["LEONIDA_ASYNC_SUBMIT"], "config");
        assert_eq!(plan.settings["LEONIDA_QUEUE2"], "0");
        assert_eq!(plan.sources["LEONIDA_QUEUE2"], "environment");
        assert_eq!(plan.settings["LEONIDA_COPY_PLAIN"], "");
        assert_eq!(plan.settings["LEONIDA_JOIN_PARTS"], "alt");
        assert_eq!(plan.settings["LEONIDA_CAPTURE"], "C:/測定/capture");
        assert!(!plan.settings.contains_key("LEONIDA_CONFIG"));
        assert!(!plan.settings.contains_key("LEONIDA_D3D12_WARP"));
    }

    #[test]
    fn local_order_null_and_user_overrides_preserve_all_layers() {
        let local = local(json!([
            { "name": "first", "env": { "LEONIDA_QUEUE2": "2", "LEONIDA_INDIRECT_SINGLE": "1" } },
            { "name": "last", "env": { "LEONIDA_QUEUE2": null, "LEONIDA_INDIRECT_SINGLE": "0", "LEONIDA_COPY_PLAIN": null } }
        ]));
        let settings = GraphicsSettings {
            queue2: Some(3),
            ..Default::default()
        };
        let plan = resolve(
            &settings,
            gpu(),
            &env(&[("LEONIDA_INDIRECT_SINGLE", "1")]),
            Some(&local),
        )
        .unwrap();
        assert_eq!(plan.profiles, ["base", "first", "last"]);
        assert_eq!(plan.settings["LEONIDA_QUEUE2"], "3");
        assert_eq!(plan.sources["LEONIDA_QUEUE2"], "config");
        assert_eq!(plan.settings["LEONIDA_INDIRECT_SINGLE"], "1");
        assert_eq!(plan.sources["LEONIDA_INDIRECT_SINGLE"], "environment");
        assert!(!plan.settings.contains_key("LEONIDA_COPY_PLAIN"));
        assert!(!plan.sources.contains_key("LEONIDA_COPY_PLAIN"));
    }

    #[test]
    fn none_retains_toml_but_measurement_off_suppresses_it() {
        for names in [
            vec![],
            vec!["NoNe".into()],
            vec!["none".into(), "missing".into()],
        ] {
            let settings = GraphicsSettings {
                profiles: Some(names),
                queue2: Some(3),
                ..Default::default()
            };
            let plan = resolve(&settings, gpu(), &env(&[]), Some("broken json")).unwrap();
            assert!(plan.profiles.is_empty());
            assert_eq!(plan.settings, env(&[("LEONIDA_QUEUE2", "3")]));
        }
        let settings = GraphicsSettings {
            async_submit: Some(true),
            ..Default::default()
        };
        let plan = resolve(
            &settings,
            gpu(),
            &env(&[
                ("LEONIDA_GRAPHICS_OFF", "1"),
                ("LEONIDA_PROFILE", "missing"),
                ("LEONIDA_QUEUE2", "2"),
            ]),
            Some("broken json"),
        )
        .unwrap();
        assert!(plan.profiles.is_empty());
        assert_eq!(plan.settings, env(&[("LEONIDA_QUEUE2", "2")]));
    }

    #[test]
    fn empty_profile_environment_keeps_toml_selection() {
        let settings = GraphicsSettings {
            profiles: Some(vec![]),
            ..Default::default()
        };
        let plan = resolve(&settings, gpu(), &env(&[("LEONIDA_PROFILE", "")]), None).unwrap();
        assert!(plan.settings.is_empty());
        // PowerShell ignores empty entries in a comma-separated override.
        let plan = resolve(
            &settings,
            gpu(),
            &env(&[("LEONIDA_PROFILE", " , , ")]),
            None,
        )
        .unwrap();
        assert_eq!(plan.profiles, ["base"]);
    }

    #[test]
    fn sampling_profiler_switch_does_not_select_a_graphics_profile() {
        let environment = env(&[("LEONIDA_PROFILE", "1")]);
        let plan = resolve(&GraphicsSettings::default(), gpu(), &environment, None).unwrap();
        assert_eq!(plan.profiles, ["base"]);
        assert!(!plan.settings.contains_key("LEONIDA_PROFILE"));
        for (names, expected) in [
            (vec![], vec![]),
            (vec!["card-windows".into()], vec!["base", "card-windows"]),
        ] {
            let settings = GraphicsSettings {
                profiles: Some(names),
                async_submit: Some(false),
                ..Default::default()
            };
            let plan = resolve(&settings, gpu(), &environment, None).unwrap();
            assert_eq!(plan.profiles, expected);
            assert_eq!(plan.settings["LEONIDA_ASYNC_SUBMIT"], "0");
        }
        assert_eq!(environment["LEONIDA_PROFILE"], "1");
    }

    #[test]
    fn local_catalog_follows_config_and_can_be_bypassed_for_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("custom.toml");
        let path = dir.path().join("gpu-profiles.local.json");
        std::fs::write(
            &path,
            local(json!([
                { "name": "beside-config", "env": { "LEONIDA_INDIRECT_SINGLE": "1" } }
            ])),
        )
        .unwrap();
        let plan = load(
            &GraphicsSettings::default(),
            Some(&source),
            gpu(),
            &env(&[]),
        )
        .unwrap();
        assert_eq!(plan.settings["LEONIDA_INDIRECT_SINGLE"], "1");
        std::fs::write(&path, "broken json").unwrap();
        assert!(
            load(
                &GraphicsSettings::default(),
                Some(&source),
                gpu(),
                &env(&[])
            )
            .is_err()
        );
        for value in [
            env(&[("LEONIDA_GRAPHICS_OFF", "1")]),
            env(&[("LEONIDA_PROFILE", "none")]),
        ] {
            let plan = load(&GraphicsSettings::default(), Some(&source), gpu(), &value).unwrap();
            assert!(plan.settings.is_empty());
        }
    }

    #[test]
    fn matching_uses_vendor_inclusive_vram_bounds_and_name() {
        let local = local(json!([
            { "name": "fits", "match": { "vendor": "NVIDIA", "min_vram_mb": 32768,
              "max_vram_mb": 32768, "name": "*RTX [45]0?0" }, "env": {} },
            { "name": "small", "match": { "max_vram_mb": 32767 }, "env": {} },
            { "name": "large", "match": { "min_vram_mb": 32769 }, "env": {} },
            { "name": "other", "match": { "vendor": "amd" }, "env": {} },
            { "name": "opt-in", "opt_in": true, "env": { "LEONIDA_INDIRECT_SINGLE": "1" } }
        ]));
        let plan = resolve(&GraphicsSettings::default(), gpu(), &env(&[]), Some(&local)).unwrap();
        assert_eq!(plan.profiles, ["base", "fits"]);
        let plan = resolve(
            &GraphicsSettings::default(),
            None,
            &env(&[("LEONIDA_PROFILE", "OpT-In")]),
            Some(&local),
        )
        .unwrap();
        assert_eq!(plan.profiles, ["base", "opt-in"]);
        assert_eq!(plan.settings["LEONIDA_INDIRECT_SINGLE"], "1");
    }

    #[test]
    fn unknown_adapter_only_matches_unqualified_profiles() {
        let local = local(json!([
            { "name": "unqualified", "env": {} },
            { "name": "vendor", "match": { "vendor": "other" }, "env": {} },
            { "name": "name", "match": { "name": "*" }, "env": {} },
            { "name": "min", "match": { "min_vram_mb": 0 }, "env": {} },
            { "name": "max", "match": { "max_vram_mb": 99999 }, "env": {} }
        ]));
        let plan = resolve(&GraphicsSettings::default(), None, &env(&[]), Some(&local)).unwrap();
        assert_eq!(plan.profiles, ["base", "unqualified"]);
    }

    #[test]
    fn wildcard_syntax_is_case_insensitive_and_checks_whole_names() {
        for (pattern, name, expected) in [
            ("", "", true),
            ("*", "", true),
            ("?", "", false),
            ("*RTX [3-5]0?0*", "Nvidia RTX 5090", true),
            ("*RTX [3-5]0?0*", "Nvidia RTX 6090", false),
            ("RTX", "RTX 5090", false),
            ("R?X", "rtx", true),
            ("*[[]test]", "card[test]", true),
            ("[a-c-]", "-", true),
            ("**a*b*c", "aaabbbc", true),
            ("*a*b", "ba", false),
            ("*É*", "café", true),
        ] {
            assert_eq!(
                wildcard(pattern, name).unwrap(),
                expected,
                "{pattern:?}, {name:?}"
            );
        }
        for pattern in ["[", "[]", "[z-a]", "[!a]", "[^a]", "`*"] {
            assert!(wildcard(pattern, "anything").is_err(), "{pattern}");
        }
    }

    #[test]
    fn unknown_profiles_and_invalid_catalog_entries_are_errors() {
        assert!(
            resolve(
                &GraphicsSettings::default(),
                gpu(),
                &env(&[("LEONIDA_PROFILE", "missing")]),
                None
            )
            .is_err()
        );
        for key in [
            "PATH",
            "LEONIDA_CONFIG",
            "LEONIDA_GAME_DIR",
            "LEONIDA_CACHE_DIR",
            "LEONIDA_DATA_DIR",
            "LEONIDA_LOG",
            "LEONIDA_PROFILE",
            "LEONIDA_TEST_TITLES",
            "LEONIDA_KEEP_WINDOW",
            "LEONIDA_GRAPHICS_OFF",
            "LEONIDA_D3D12_WARP",
            "LEONIDA_RENDERDOC",
            "LEONIDA_RENDERDOC_OUT",
            "LEONIDA_RELAUNCHED_FROM",
            "LEONIDA_AUDIO_CAPTURE_OF",
            "LEONIDA_bad",
            "LEONIDA_X=Y",
            "LEONIDA_",
        ] {
            let local = local(json!([{ "name": "bad", "opt_in": true, "env": { (key): "1" } }]));
            assert!(
                resolve(&GraphicsSettings::default(), gpu(), &env(&[]), Some(&local)).is_err(),
                "{key}"
            );
        }
        for profile in [
            json!({ "name": "", "env": {} }),
            json!({ "name": "bad", "env": { "LEONIDA_QUEUE2": "\u{0}" } }),
            json!({ "name": "bad", "match": { "min_vram_mb": 2, "max_vram_mb": 1 }, "env": {} }),
            json!({ "name": "bad", "match": { "name": "[" }, "env": {} }),
            json!({ "name": "bad", "match": { "vendor": "nvida" }, "env": {} }),
            json!({ "name": "bad", "opt-in": true, "env": {} }),
        ] {
            assert!(
                resolve(
                    &GraphicsSettings::default(),
                    gpu(),
                    &env(&[]),
                    Some(&local(json!([profile])))
                )
                .is_err()
            );
        }
    }

    #[test]
    fn toml_ranges_types_and_unknown_fields_are_checked() {
        for text in [
            "queue2 = 4",
            "card_windows_mb = 17592186044416",
            "texture_room_percent = 49",
            "texture_room_percent = 98",
            "profiles = ['']",
        ] {
            let settings: GraphicsSettings = toml::from_str(text).unwrap();
            assert!(settings.validate().is_err(), "{text}");
        }
        for text in [
            "queue2 = -1",
            "async_submit = 1",
            "copy_plian = true",
            "profiles = 'base'",
        ] {
            assert!(toml::from_str::<GraphicsSettings>(text).is_err(), "{text}");
        }
        for text in [
            "queue2 = 0",
            "queue2 = 3",
            "card_windows_mb = 17592186044415",
            "texture_room_percent = 50",
            "texture_room_percent = 97",
        ] {
            toml::from_str::<GraphicsSettings>(text)
                .unwrap()
                .validate()
                .unwrap();
        }
    }

    #[test]
    fn all_toml_switches_have_the_expected_environment_mapping() {
        let settings: GraphicsSettings = toml::from_str(
            "profiles = []\nasync_submit = false\nqueue2 = 3\nindirect_window = true\n\
             copy_plain = false\njoin_parts = true\nback_own_view = false\n\
             indirect_gather = true\ncard_rehash_runs = false\nwindow_upload = true\n\
             card_windows_mb = 4096\ntexture_room_percent = 97\n",
        )
        .unwrap();
        let plan = resolve(&settings, None, &env(&[]), None).unwrap();
        assert_eq!(
            plan.settings,
            env(&[
                ("LEONIDA_ASYNC_SUBMIT", "0"),
                ("LEONIDA_QUEUE2", "3"),
                ("LEONIDA_INDIRECT_WINDOW", "1"),
                ("LEONIDA_COPY_PLAIN", "0"),
                ("LEONIDA_JOIN_PARTS", "1"),
                ("LEONIDA_BACK_OWN_VIEW", "0"),
                ("LEONIDA_INDIRECT_GATHER", "1"),
                ("LEONIDA_CARD_REHASH_RUNS", "0"),
                ("LEONIDA_WINDOW_UPLOAD", "1"),
                ("LEONIDA_CARD_WINDOWS_MB", "4096"),
                ("LEONIDA_TEXTURE_ROOM_PERCENT", "97"),
            ])
        );
        assert!(plan.sources.values().all(|source| source == "config"));
        let json: serde_json::Value = serde_json::from_str(&plan.to_json().unwrap()).unwrap();
        assert!(json["gpu"].is_null());
        assert_eq!(json["settings"]["LEONIDA_QUEUE2"], "3");
    }

    #[test]
    fn environment_snapshot_preserves_empty_and_unicode_values() {
        let snapshot = snapshot_from([
            ("leonida_queue2".into(), "".into()),
            ("LEONIDA_CAPTURE".into(), "C:/測定".into()),
            ("LEONIDA_PROFILE".into(), "base".into()),
            ("PATH".into(), "unrelated".into()),
        ])
        .unwrap();
        assert_eq!(
            snapshot,
            env(&[
                ("LEONIDA_QUEUE2", ""),
                ("LEONIDA_CAPTURE", "C:/測定"),
                ("LEONIDA_PROFILE", "base"),
            ])
        );
    }

    #[test]
    fn environment_snapshot_rejects_non_unicode_switches() {
        #[cfg(unix)]
        let bad = {
            use std::os::unix::ffi::OsStringExt;
            std::ffi::OsString::from_vec(vec![0xff])
        };
        #[cfg(windows)]
        let bad = {
            use std::os::windows::ffi::OsStringExt;
            std::ffi::OsString::from_wide(&[0xd800])
        };
        assert!(snapshot_from([("LEONIDA_QUEUE2".into(), bad.clone())]).is_err());
        assert!(snapshot_from([("LEONIDA_PROFILE".into(), bad.clone())]).is_err());
        // Unrelated variables and paths retain their OS-specific handling.
        assert!(
            snapshot_from([
                ("PATH".into(), bad.clone()),
                ("LEONIDA_GAME_DIR".into(), bad)
            ])
            .is_ok()
        );
    }
}
