//! Pure, IO-free classpath index and JEP 238 Multi-Release JAR entry selection.
//! See `docs/specs/0006-jdk-and-jar-classpath-indexing.md`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use smol_str::SmolStr;
use syntax::Release;

use crate::symbols::{MemberKind, ParamSummary, TypeKind};

/// Where a binary type originated (used for hover display and on-demand source/stub lookup).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BinaryOrigin {
    /// A JPMS JDK module (e.g. `module_name = Some("java.base")`),
    /// or JDK 8 `rt.jar` (`module_name = None`).
    Jdk { module_name: Option<SmolStr> },
    /// A local `.jar` archive or compiled classes directory.
    Jar {
        /// Display name / normalized path (e.g. `"lib/junit-4.13.2.jar"`).
        label: String,
    },
}

/// Identifies a resolved binary type or member for `hover` and `goto_definition`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BinaryTarget {
    pub origin: BinaryOrigin,
    /// JVM internal binary name (e.g. `"java/util/Map$Entry"`).
    pub internal_name: String,
    /// Canonical dot-separated FQN (e.g. `"java.util.Map.Entry"`).
    pub qualified_name: String,
    /// Member simple name when targeting a field, method, constructor, enum constant, or record component.
    pub member_name: Option<SmolStr>,
}

/// A compiled Java type parsed from a `.class` file (no source `FileId` / `TextRange`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryTypeSymbol {
    /// Simple name (e.g. `"String"`, `"Entry"`).
    pub name: SmolStr,
    /// Canonical dot-separated FQN (e.g. `"java.lang.String"`, `"java.util.Map.Entry"`).
    pub qualified_name: String,
    /// JVM internal binary name (e.g. `"java/util/Map$Entry"`), used for `src.zip` & stub lookup.
    pub internal_name: String,
    /// Package name (`""` for unnamed package, `"java.lang"` otherwise).
    pub package: String,
    /// Enclosing type's canonical FQN if nested (e.g. `Some("java.util.Map")`).
    pub enclosing_type: Option<String>,
    pub kind: TypeKind,
    pub is_public: bool,
    pub is_static: bool,
    pub is_final: bool,
    pub is_abstract: bool,
    /// Type parameters from generic `Signature` attribute (e.g. `["E"]`, `["K", "V"]`).
    pub type_params: Vec<SmolStr>,
    /// Superclass canonical FQN (e.g. `Some("java.lang.Object")`; `None` for `java.lang.Object`).
    pub super_class: Option<String>,
    /// Directly implemented/extended interface FQNs.
    pub interfaces: Vec<String>,
    /// Sealed `permits` FQNs from `PermittedSubclasses` attribute (Java 17+).
    pub permitted_subclasses: Vec<String>,
    /// Public/protected fields, methods, constructors, enum constants, and record components.
    pub members: Vec<BinaryMemberSymbol>,
    /// Simple names of directly nested public/protected types.
    pub nested_types: Vec<SmolStr>,
    pub origin: BinaryOrigin,
}

impl BinaryTypeSymbol {
    /// Builds a [`BinaryTarget`] pointing at this type or one of its members.
    #[must_use]
    pub fn to_target(&self, member_name: Option<SmolStr>) -> BinaryTarget {
        BinaryTarget {
            origin: self.origin.clone(),
            internal_name: self.internal_name.clone(),
            qualified_name: self.qualified_name.clone(),
            member_name,
        }
    }
}

/// A public or protected member parsed from a `.class` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryMemberSymbol {
    pub owner_type: String,
    pub name: SmolStr,
    pub kind: MemberKind,
    pub is_public: bool,
    pub is_static: bool,
    /// Human-readable return or field type (decoded from generic `Signature` if present, else JVM descriptor).
    pub type_ref: Option<String>,
    /// Method/constructor parameters (using `MethodParameters` names when present, else `arg0`, `arg1`, ...).
    pub params: Vec<ParamSummary>,
    pub is_varargs: bool,
}

/// Parsed JPMS module exports from `module-info.class` (used for `import module M;`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BinaryModuleInfo {
    pub name: SmolStr,
    /// Unqualified exported packages (e.g. `["java.lang", "java.util", ...]`).
    pub exports: Vec<String>,
    /// `requires transitive` target module names.
    pub requires_transitive: Vec<SmolStr>,
}

/// Pure, deterministic index of all binary JDK and JAR types visible to the workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClasspathIndex {
    /// Canonical FQN (`"java.lang.String"`, `"java.util.Map.Entry"`) -> `BinaryTypeSymbol`.
    pub types_by_fqn: BTreeMap<String, BinaryTypeSymbol>,
    /// Package name (`"java.lang"`) -> sorted top-level type FQNs.
    pub types_by_package: BTreeMap<String, Vec<String>>,
    /// Package name -> (simple name -> FQN) for O(log N) lookups.
    pub package_type_map: BTreeMap<String, BTreeMap<SmolStr, String>>,
    /// JPMS module name (`"java.base"`) -> `BinaryModuleInfo`.
    pub modules: BTreeMap<SmolStr, BinaryModuleInfo>,
}

impl ClasspathIndex {
    /// Inserts a parsed binary type into the index, updating package lookup tables for top-level types.
    pub fn insert_type(&mut self, ty: BinaryTypeSymbol) {
        let fqn = ty.qualified_name.clone();
        let is_top_level = ty.enclosing_type.is_none();
        let pkg = ty.package.clone();
        let simple = ty.name.clone();

        let already_present = self.types_by_fqn.insert(fqn.clone(), ty).is_some();

        if is_top_level {
            if !already_present {
                self.types_by_package
                    .entry(pkg.clone())
                    .or_default()
                    .push(fqn.clone());
            }
            self.package_type_map
                .entry(pkg)
                .or_default()
                .insert(simple, fqn);
        }
    }

    /// Inserts a parsed JPMS module descriptor into the index.
    pub fn insert_module(&mut self, module: BinaryModuleInfo) {
        self.modules.insert(module.name.clone(), module);
    }

    /// Merges another [`ClasspathIndex`] into `self`.
    pub fn merge(&mut self, other: ClasspathIndex) {
        for (_, ty) in other.types_by_fqn {
            self.insert_type(ty);
        }
        for (_, module) in other.modules {
            self.insert_module(module);
        }
    }

    /// Finalizes the index after batch insertion:
    /// - populates `nested_types` on enclosing types from indexed nested types;
    /// - sorts and deduplicates `types_by_package` and `nested_types`.
    pub fn finalize(&mut self) {
        let mut nested_by_enclosing: BTreeMap<String, Vec<SmolStr>> = BTreeMap::new();
        for ty in self.types_by_fqn.values() {
            if let Some(enclosing) = &ty.enclosing_type {
                nested_by_enclosing
                    .entry(enclosing.clone())
                    .or_default()
                    .push(ty.name.clone());
            }
        }

        for (enclosing_fqn, children) in nested_by_enclosing {
            if let Some(parent) = self.types_by_fqn.get_mut(&enclosing_fqn) {
                parent.nested_types.extend(children);
            }
        }

        for ty in self.types_by_fqn.values_mut() {
            if ty.nested_types.len() > 1 {
                ty.nested_types.sort();
                ty.nested_types.dedup();
            }
        }

        for fqns in self.types_by_package.values_mut() {
            fqns.sort();
            fqns.dedup();
        }
    }

    /// Looks up a binary type by its canonical dot-separated FQN.
    #[must_use]
    pub fn type_by_fqn(&self, fqn: &str) -> Option<&BinaryTypeSymbol> {
        self.types_by_fqn.get(fqn)
    }

    /// Looks up a top-level binary type by package name and simple name.
    #[must_use]
    pub fn type_in_package(&self, pkg: &str, simple_name: &str) -> Option<&BinaryTypeSymbol> {
        let fqn = self.package_type_map.get(pkg)?.get(simple_name)?;
        self.types_by_fqn.get(fqn)
    }

    /// Returns `true` if `prefix` is a known package or ancestor prefix of any indexed package.
    #[must_use]
    pub fn has_package(&self, prefix: &str) -> bool {
        if prefix.is_empty() {
            return false;
        }
        if self.types_by_package.contains_key(prefix) {
            return true;
        }
        let prefix_dot = format!("{prefix}.");
        self.types_by_package
            .range(prefix_dot.clone()..)
            .next()
            .is_some_and(|(pkg, _)| pkg.starts_with(&prefix_dot))
    }

    /// Returns all packages exported by `module_name` and (transitively, cycle-safely) by any
    /// `requires transitive` dependencies.
    #[must_use]
    pub fn transitive_module_exports(&self, module_name: &str) -> Vec<&str> {
        let Some(root) = self.modules.get(module_name) else {
            return Vec::new();
        };

        let mut visited_modules: BTreeSet<&str> = BTreeSet::new();
        let mut seen_packages: BTreeSet<&str> = BTreeSet::new();
        let mut exports = Vec::new();
        let mut queue = VecDeque::new();

        visited_modules.insert(root.name.as_str());
        queue.push_back(root);

        while let Some(module) = queue.pop_front() {
            for pkg in &module.exports {
                if seen_packages.insert(pkg.as_str()) {
                    exports.push(pkg.as_str());
                }
            }
            for dep_name in &module.requires_transitive {
                if visited_modules.insert(dep_name.as_str())
                    && let Some(dep_module) = self.modules.get(dep_name.as_str())
                {
                    queue.push_back(dep_module);
                }
            }
        }

        exports
    }

    /// Returns the total number of indexed binary types (top-level and nested).
    #[must_use]
    pub fn type_count(&self) -> usize {
        self.types_by_fqn.len()
    }

    /// Returns the total number of indexed JPMS modules.
    #[must_use]
    pub fn module_count(&self) -> usize {
        self.modules.len()
    }
}

/// Parses `META-INF/MANIFEST.MF` and returns `true` iff `Multi-Release: true` is present.
#[must_use]
pub fn parse_manifest_multi_release(manifest_text: &str) -> bool {
    // Unfold RFC 822 / JAR manifest continuation lines (lines starting with a single space).
    let mut headers: Vec<String> = Vec::new();
    for raw_line in manifest_text.lines() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if let Some(cont) = line.strip_prefix(' ') {
            if let Some(last) = headers.last_mut() {
                last.push_str(cont);
            }
        } else if !line.is_empty() {
            headers.push(line.to_string());
        }
    }

    headers.iter().any(|header| {
        if let Some((key, value)) = header.split_once(':') {
            key.trim().eq_ignore_ascii_case("Multi-Release")
                && value.trim().eq_ignore_ascii_case("true")
        } else {
            false
        }
    })
}

/// Selects the effective `.class` entry bytes for a JAR given `is_multi_release` and `target_release`.
///
/// Entries are `(zip_entry_path, bytes)`:
/// - Base entries live at `<pkg>/<Name>.class` (and `module-info.class`).
/// - When `is_multi_release == true`, entries under `META-INF/versions/<N>/<rel>` with
///   `9 <= N <= target_release.number()` override base entries in ascending order of `N`.
/// - Entries with `N > target_release.number()` are ignored.
#[must_use]
pub fn select_mrjar_entries<'a>(
    entries: &'a [(&'a str, &'a [u8])],
    is_multi_release: bool,
    target_release: Release,
) -> BTreeMap<String, &'a [u8]> {
    let mut selected = BTreeMap::new();
    let mut versioned: BTreeMap<u16, Vec<(&'a str, &'a [u8])>> = BTreeMap::new();
    let max_release = target_release.number();

    for &(raw_path, bytes) in entries {
        let path = raw_path.strip_prefix('/').unwrap_or(raw_path);
        if !path.ends_with(".class") {
            continue;
        }

        if let Some(rest) = path.strip_prefix("META-INF/versions/") {
            if !is_multi_release {
                continue;
            }
            let Some((ver_str, rel_path)) = rest.split_once('/') else {
                continue;
            };
            if rel_path.is_empty() {
                continue;
            }
            let Ok(ver) = ver_str.parse::<u16>() else {
                continue;
            };
            if (9..=max_release).contains(&ver) {
                versioned.entry(ver).or_default().push((rel_path, bytes));
            }
        } else if !path.starts_with("META-INF/") {
            selected.insert(path.to_string(), bytes);
        }
    }

    for (_ver, ver_entries) in versioned {
        for (rel_path, bytes) in ver_entries {
            selected.insert(rel_path.to_string(), bytes);
        }
    }

    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_type(fqn: &str, pkg: &str, name: &str, enclosing: Option<&str>) -> BinaryTypeSymbol {
        BinaryTypeSymbol {
            name: SmolStr::new(name),
            qualified_name: fqn.to_string(),
            internal_name: fqn.replace('.', "/"),
            package: pkg.to_string(),
            enclosing_type: enclosing.map(str::to_string),
            kind: TypeKind::Class,
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: false,
            type_params: Vec::new(),
            super_class: Some("java.lang.Object".to_string()),
            interfaces: Vec::new(),
            permitted_subclasses: Vec::new(),
            members: Vec::new(),
            nested_types: Vec::new(),
            origin: BinaryOrigin::Jdk {
                module_name: Some(SmolStr::new("java.base")),
            },
        }
    }

    #[test]
    fn parse_manifest_multi_release_handles_true_false_and_continuations() {
        assert!(parse_manifest_multi_release(
            "Manifest-Version: 1.0\r\nMulti-Release: true\r\n"
        ));
        assert!(parse_manifest_multi_release("multi-release: TRUE\n"));
        assert!(parse_manifest_multi_release(
            "Manifest-Version: 1.0\nMulti-Release:\n true\n"
        ));
        assert!(!parse_manifest_multi_release(
            "Manifest-Version: 1.0\nMulti-Release: false\n"
        ));
        assert!(!parse_manifest_multi_release("Manifest-Version: 1.0\n"));
    }

    #[test]
    fn select_mrjar_entries_applies_version_overlays_by_target_release() {
        let base_foo: &[u8] = b"base-foo";
        let v9_foo: &[u8] = b"v9-foo";
        let v17_foo: &[u8] = b"v17-foo";
        let v21_foo: &[u8] = b"v21-foo";
        let v9_mod: &[u8] = b"v9-mod";
        let base_bar: &[u8] = b"base-bar";
        let manifest: &[u8] = b"Multi-Release: true";

        let entries = [
            ("META-INF/MANIFEST.MF", manifest),
            ("com/example/Foo.class", base_foo),
            ("com/example/Bar.class", base_bar),
            ("META-INF/versions/9/com/example/Foo.class", v9_foo),
            ("META-INF/versions/9/module-info.class", v9_mod),
            ("META-INF/versions/17/com/example/Foo.class", v17_foo),
            ("META-INF/versions/21/com/example/Foo.class", v21_foo),
        ];

        // JAVA_8 ignores all META-INF/versions/* overlays.
        let java8 = select_mrjar_entries(&entries, true, Release::JAVA_8);
        assert_eq!(java8.get("com/example/Foo.class").copied(), Some(base_foo));
        assert_eq!(java8.get("com/example/Bar.class").copied(), Some(base_bar));
        assert!(!java8.contains_key("module-info.class"));
        assert!(!java8.contains_key("META-INF/MANIFEST.MF"));

        // JAVA_17 applies 9 and 17, ignores 21.
        let java17 = select_mrjar_entries(&entries, true, Release::JAVA_17);
        assert_eq!(java17.get("com/example/Foo.class").copied(), Some(v17_foo));
        assert_eq!(java17.get("com/example/Bar.class").copied(), Some(base_bar));
        assert_eq!(java17.get("module-info.class").copied(), Some(v9_mod));

        // JAVA_21 applies 9, 17, 21.
        let java21 = select_mrjar_entries(&entries, true, Release::JAVA_21);
        assert_eq!(java21.get("com/example/Foo.class").copied(), Some(v21_foo));

        // Multi-Release: false ignores all META-INF/versions/* even on JAVA_21.
        let non_mr = select_mrjar_entries(&entries, false, Release::JAVA_21);
        assert_eq!(non_mr.get("com/example/Foo.class").copied(), Some(base_foo));
        assert!(!non_mr.contains_key("module-info.class"));
    }

    #[test]
    fn classpath_index_insert_merge_finalize_and_transitive_modules() {
        let mut a = ClasspathIndex::default();
        a.insert_type(dummy_type("java.util.Map", "java.util", "Map", None));
        a.insert_type(dummy_type(
            "java.util.Map.Entry",
            "java.util",
            "Entry",
            Some("java.util.Map"),
        ));

        let mut b = ClasspathIndex::default();
        b.insert_type(dummy_type("java.lang.String", "java.lang", "String", None));

        // Cyclic module graph: java.se -> java.sql -> java.xml -> java.sql, plus java.base.
        a.insert_module(BinaryModuleInfo {
            name: SmolStr::new("java.se"),
            exports: vec![],
            requires_transitive: vec![SmolStr::new("java.sql"), SmolStr::new("java.base")],
        });
        a.insert_module(BinaryModuleInfo {
            name: SmolStr::new("java.sql"),
            exports: vec!["java.sql".to_string()],
            requires_transitive: vec![SmolStr::new("java.xml")],
        });
        a.insert_module(BinaryModuleInfo {
            name: SmolStr::new("java.xml"),
            exports: vec!["javax.xml".to_string()],
            requires_transitive: vec![SmolStr::new("java.sql")],
        });
        b.insert_module(BinaryModuleInfo {
            name: SmolStr::new("java.base"),
            exports: vec!["java.lang".to_string(), "java.util".to_string()],
            requires_transitive: vec![],
        });

        a.merge(b);
        a.finalize();

        assert_eq!(a.type_count(), 3);
        assert_eq!(a.module_count(), 4);
        assert!(a.has_package("java"));
        assert!(a.has_package("java.util"));
        assert!(!a.has_package("java.ut"));

        let map = a.type_by_fqn("java.util.Map").unwrap();
        assert_eq!(map.nested_types, vec![SmolStr::new("Entry")]);
        assert_eq!(
            a.type_in_package("java.util", "Map").map(|t| &t.name),
            Some(&SmolStr::new("Map"))
        );
        // Nested types are not top-level package entries.
        assert!(a.type_in_package("java.util", "Entry").is_none());

        let exports = a.transitive_module_exports("java.se");
        assert_eq!(exports, vec!["java.sql", "java.lang", "java.util", "javax.xml"]);
        assert!(a.transitive_module_exports("missing.module").is_empty());
    }
}
