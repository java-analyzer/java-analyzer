use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use ide::{normalize_rel_path, DeltaSet, Release, WorkspaceDelta};
use serde::{Deserialize, Serialize};

use super::{
    build_tool_name, parse_build_tool_kind, parse_version_string, ExplicitConfig, WorkspaceProbe,
};

pub const JAVA_ANALYZER_DOT_CONFIG_FILE: &str = ".java-analyzer.toml";
pub const JAVA_ANALYZER_PLAIN_CONFIG_FILE: &str = "java-analyzer.toml";

pub fn find_config_file(workspace_dir: &Path) -> Option<PathBuf> {
    let dot_file = workspace_dir.join(JAVA_ANALYZER_DOT_CONFIG_FILE);
    if dot_file.is_file() {
        return Some(dot_file);
    }

    let plain_file = workspace_dir.join(JAVA_ANALYZER_PLAIN_CONFIG_FILE);
    if plain_file.is_file() {
        return Some(plain_file);
    }

    None
}

// ---- Serde DTOs for `.java-analyzer.toml` ----

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum ReleaseValue {
    Number(u16),
    Text(String),
}

impl ReleaseValue {
    fn to_release(&self) -> Option<Release> {
        match self {
            Self::Number(n) => parse_version_string(&n.to_string()),
            Self::Text(s) => parse_version_string(s),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum ClasspathCommand {
    Args(Vec<String>),
    CommandLine(String),
}

impl ClasspathCommand {
    fn into_args(self) -> Vec<String> {
        match self {
            Self::Args(args) => args,
            Self::CommandLine(cmd) => cmd.split_whitespace().map(ToString::to_string).collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct RawDeltaSet {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    include: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    exclude: Vec<String>,
}

impl RawDeltaSet {
    fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    fn into_delta_set(self, normalize_paths: bool) -> DeltaSet<String> {
        let map_items = |items: Vec<String>| -> Vec<String> {
            if normalize_paths {
                items.into_iter().map(|p| normalize_rel_path(&p)).collect()
            } else {
                items
            }
        };
        DeltaSet {
            include: map_items(self.include),
            exclude: map_items(self.exclude),
        }
    }

    fn from_delta_set(delta: &DeltaSet<String>) -> Self {
        Self {
            include: delta.include.clone(),
            exclude: delta.exclude.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct RawModuleConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    release: Option<ReleaseValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preview: Option<bool>,
    #[serde(default, skip_serializing_if = "RawDeltaSet::is_empty")]
    sources: RawDeltaSet,
    #[serde(default, skip_serializing_if = "RawDeltaSet::is_empty")]
    resources: RawDeltaSet,
    #[serde(default, skip_serializing_if = "RawDeltaSet::is_empty")]
    dependencies: RawDeltaSet,
}

impl RawModuleConfig {
    fn is_empty(&self) -> bool {
        self.release.is_none()
            && self.preview.is_none()
            && self.sources.is_empty()
            && self.resources.is_empty()
            && self.dependencies.is_empty()
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct RawConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    build: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    jdk: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    release: Option<ReleaseValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preview: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    classpath_command: Option<ClasspathCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    include: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    exclude: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    modules: BTreeMap<String, RawModuleConfig>,
}

// ---- Minimal-Overlay `.java-analyzer.toml` Parser & Serializer ----

pub fn parse_config_toml(text: &str) -> ExplicitConfig {
    let raw = match toml::from_str::<RawConfig>(text) {
        Ok(raw) => raw,
        Err(err) => {
            tracing::warn!(%err, "failed to parse workspace TOML config; falling back to defaults");
            return ExplicitConfig::default();
        }
    };

    let mut cfg = ExplicitConfig {
        build: raw.build.as_deref().and_then(parse_build_tool_kind),
        jdk: raw.jdk,
        release: raw.release.as_ref().and_then(ReleaseValue::to_release),
        preview: raw.preview,
        classpath_command: raw
            .classpath_command
            .map(ClasspathCommand::into_args)
            .unwrap_or_default(),
        delta: WorkspaceDelta {
            include: raw
                .include
                .into_iter()
                .map(|p| normalize_rel_path(&p))
                .collect(),
            exclude: raw
                .exclude
                .into_iter()
                .map(|p| normalize_rel_path(&p))
                .collect(),
            modules: BTreeMap::new(),
        },
    };

    for (mod_name, raw_mod) in raw.modules {
        let norm_mod = if mod_name.contains('/') || mod_name == "." {
            normalize_rel_path(&mod_name)
        } else {
            mod_name
        };
        let entry = cfg.delta.modules.entry(norm_mod).or_default();
        if let Some(rel) = raw_mod.release.as_ref().and_then(ReleaseValue::to_release) {
            entry.release = Some(rel);
        }
        if let Some(prev) = raw_mod.preview {
            entry.preview = Some(prev);
        }
        if !raw_mod.sources.is_empty() {
            entry.sources = raw_mod.sources.into_delta_set(true);
        }
        if !raw_mod.resources.is_empty() {
            entry.resources = raw_mod.resources.into_delta_set(true);
        }
        if !raw_mod.dependencies.is_empty() {
            entry.dependencies = raw_mod.dependencies.into_delta_set(false);
        }
    }

    cfg
}

/// Serializes `config` into a minimal-overlay `.java-analyzer.toml` string against `probe`.
/// Any field in `config` that duplicates what `probe` already uniquely inferred from Tier 2 / Tier 3
/// is omitted so `.java-analyzer.toml` never hardcodes externally declared facts.
pub fn render_minimal_toml(probe: &WorkspaceProbe, config: &ExplicitConfig) -> String {
    let build = config
        .build
        .filter(|&b| probe.inferred_build != Some(b))
        .map(|b| build_tool_name(b).to_string());

    let inferred_rel = probe.inferred_level.map(|l| l.release);
    let release = config
        .release
        .filter(|&r| inferred_rel != Some(r))
        .map(|r| ReleaseValue::Number(r.number()));

    let inferred_prev = probe.inferred_level.map(|l| l.preview).unwrap_or(false);
    let preview = config.preview.filter(|&p| p != inferred_prev);

    let classpath_command = (!config.classpath_command.is_empty())
        .then(|| ClasspathCommand::Args(config.classpath_command.clone()));

    let mut modules = BTreeMap::new();
    for (mod_key, mod_cfg) in &config.delta.modules {
        let raw_mod = RawModuleConfig {
            release: mod_cfg.release.map(|r| ReleaseValue::Number(r.number())),
            preview: mod_cfg.preview.filter(|&p| p),
            sources: RawDeltaSet::from_delta_set(&mod_cfg.sources),
            resources: RawDeltaSet::from_delta_set(&mod_cfg.resources),
            dependencies: RawDeltaSet::from_delta_set(&mod_cfg.dependencies),
        };
        if !raw_mod.is_empty() {
            modules.insert(mod_key.clone(), raw_mod);
        }
    }

    let raw = RawConfig {
        build,
        jdk: config.jdk.clone(),
        release,
        preview,
        classpath_command,
        include: config.delta.include.clone(),
        exclude: config.delta.exclude.clone(),
        modules,
    };

    toml::to_string(&raw).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_config::BuildToolKind;
    use ide::{LanguageLevel, ModuleDelta, WorkspaceLayout};

    fn dummy_probe(
        inferred_build: Option<BuildToolKind>,
        inferred_level: Option<LanguageLevel>,
    ) -> WorkspaceProbe {
        WorkspaceProbe {
            workspace_dir: PathBuf::from("."),
            config_file: None,
            explicit_config: None,
            build_specs: Vec::new(),
            version_specs: Vec::new(),
            inferred_build,
            inferred_level,
            conflicts: Vec::new(),
            resolved_build: inferred_build.unwrap_or(BuildToolKind::None),
            resolved_level: inferred_level.unwrap_or_else(LanguageLevel::latest),
            resolved_jdk: None,
            layout: WorkspaceLayout {
                modules: Vec::new(),
                file_levels: BTreeMap::new(),
                package_mismatches: Vec::new(),
                authoritative_packages: BTreeMap::new(),
            },
            file_paths: BTreeMap::new(),
            file_contents: BTreeMap::new(),
        }
    }

    #[test]
    fn round_trip_scalars_and_module_tables() {
        let mut config = ExplicitConfig {
            build: Some(BuildToolKind::Gradle),
            jdk: Some(PathBuf::from("/opt/jdks/jdk-21")),
            release: Some(Release::JAVA_21),
            preview: Some(true),
            classpath_command: vec!["./gradlew".into(), "printClasspath".into()],
            delta: WorkspaceDelta {
                include: vec!["services/core".into()],
                exclude: vec!["legacy".into()],
                modules: BTreeMap::new(),
            },
        };

        config.delta.modules.insert(
            ".".into(),
            ModuleDelta {
                release: Some(Release::JAVA_17),
                preview: Some(false),
                sources: DeltaSet {
                    include: vec!["generated/src".into()],
                    exclude: vec!["src/ignored".into()],
                },
                resources: DeltaSet {
                    include: vec!["assets".into()],
                    exclude: Vec::new(),
                },
                dependencies: DeltaSet {
                    include: vec!["com.google.guava:guava:33.3.1-jre".into()],
                    exclude: Vec::new(),
                },
            },
        );
        config.delta.modules.insert(
            "services/core".into(),
            ModuleDelta {
                release: Some(Release::JAVA_25),
                preview: Some(true),
                sources: DeltaSet::default(),
                resources: DeltaSet::default(),
                dependencies: DeltaSet {
                    include: vec!["github:google/guava#v33.3.1".into()],
                    exclude: vec!["com.example:old-lib".into()],
                },
            },
        );

        let probe = dummy_probe(None, None);
        let rendered = render_minimal_toml(&probe, &config);
        let parsed = parse_config_toml(&rendered);

        assert_eq!(parsed.build, config.build);
        assert_eq!(parsed.jdk, config.jdk);
        assert_eq!(parsed.release, config.release);
        assert_eq!(parsed.preview, config.preview);
        assert_eq!(parsed.classpath_command, config.classpath_command);
        assert_eq!(parsed.delta.include, config.delta.include);
        assert_eq!(parsed.delta.exclude, config.delta.exclude);

        let root_mod = parsed.delta.modules.get(".").unwrap();
        assert_eq!(root_mod.release, Some(Release::JAVA_17));
        // `preview = false` on module is omitted during minimal rendering.
        assert_eq!(root_mod.preview, None);
        assert_eq!(root_mod.sources, config.delta.modules["."].sources);
        assert_eq!(root_mod.resources, config.delta.modules["."].resources);
        assert_eq!(
            root_mod.dependencies,
            config.delta.modules["."].dependencies
        );

        let core_mod = parsed.delta.modules.get("services/core").unwrap();
        assert_eq!(core_mod, &config.delta.modules["services/core"]);
    }

    #[test]
    fn minimal_rendering_omits_inferred_values_and_empty_tables() {
        let mut config = ExplicitConfig {
            build: Some(BuildToolKind::Maven),
            jdk: None,
            release: Some(Release::JAVA_21),
            preview: Some(false),
            classpath_command: Vec::new(),
            delta: WorkspaceDelta::default(),
        };
        config.delta.modules.insert(
            "empty-preview-false".into(),
            ModuleDelta {
                preview: Some(false),
                ..ModuleDelta::default()
            },
        );

        let probe = dummy_probe(
            Some(BuildToolKind::Maven),
            Some(LanguageLevel::new(Release::JAVA_21, false)),
        );
        assert_eq!(render_minimal_toml(&probe, &config), "");
    }

    #[test]
    fn parses_inline_and_subtable_module_syntax_and_multiline_arrays() {
        let inline_toml = r#"
include = [
    "./services/core/",
    "apps/api",
]

[modules."."]
sources = { include = ["./generated/src/"], exclude = ["src/legacy/"] }
dependencies = { include = ["com.google.guava:guava:33.3.1-jre"] }

[modules."./services/core/"]
resources = { include = ["assets/"] }
dependencies = { exclude = ["com.example:bad"] }
"#;

        let subtable_toml = r#"
include = ["./services/core/", "apps/api"]

[modules.".".sources]
include = [
    "./generated/src/",
]
exclude = ["src/legacy/"]

[modules.".".dependencies]
include = ["com.google.guava:guava:33.3.1-jre"]

[modules."services/core".resources]
include = ["assets/"]

[modules."services/core".dependencies]
exclude = ["com.example:bad"]
"#;

        let a = parse_config_toml(inline_toml);
        let b = parse_config_toml(subtable_toml);
        assert_eq!(a, b);
        assert_eq!(a.delta.include, vec!["services/core", "apps/api"]);
        assert_eq!(
            a.delta.modules["."].sources.include,
            vec!["generated/src"]
        );
        assert_eq!(a.delta.modules["."].sources.exclude, vec!["src/legacy"]);
        assert_eq!(
            a.delta.modules["."].dependencies.include,
            vec!["com.google.guava:guava:33.3.1-jre"]
        );
        assert_eq!(
            a.delta.modules["services/core"].resources.include,
            vec!["assets"]
        );
        assert_eq!(
            a.delta.modules["services/core"].dependencies.exclude,
            vec!["com.example:bad"]
        );
    }

    #[test]
    fn parses_integer_and_string_release_and_classpath_command_variants() {
        let as_int_and_array = parse_config_toml(
            r#"
release = 21
classpath_command = ["mvn", "dependency:build-classpath", "-Dmdep.outputFile=/dev/stdout"]

[modules.core]
release = 8
"#,
        );
        assert_eq!(as_int_and_array.release, Some(Release::JAVA_21));
        assert_eq!(
            as_int_and_array.classpath_command,
            vec![
                "mvn",
                "dependency:build-classpath",
                "-Dmdep.outputFile=/dev/stdout"
            ]
        );
        assert_eq!(
            as_int_and_array.delta.modules["core"].release,
            Some(Release::JAVA_8)
        );

        let as_strings = parse_config_toml(
            r#"
release = "21"
classpath_command = "mvn dependency:build-classpath -Dmdep.outputFile=/dev/stdout"

[modules.core]
release = "1.8"
"#,
        );
        assert_eq!(as_strings, as_int_and_array);
    }

    #[test]
    fn invalid_toml_falls_back_to_default() {
        assert_eq!(
            parse_config_toml("build = [unclosed\nrelease = ???"),
            ExplicitConfig::default()
        );
        assert_eq!(
            parse_config_toml("release = { invalid = true }"),
            ExplicitConfig::default()
        );
    }
}
