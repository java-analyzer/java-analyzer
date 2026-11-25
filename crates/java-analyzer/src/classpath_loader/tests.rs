use std::{
    fs,
    io::{Cursor, Write as _},
    path::{Path, PathBuf},
};

use ide::{BinaryOrigin, BinaryTarget, LanguageLevel, ModuleDependency, Release, RootScope, SmolStr};
use zip::write::SimpleFileOptions;

use super::*;
use crate::{
    test_util::TestDir,
    workspace_config::{WorkspaceProbe, discover_jdks, probe_jdk_release, probe_workspace},
};

// ---- Pure-Rust minimal .class / module-info.class byte builder (JVM-free) ----

struct ClassBuilder {
    cp: Vec<CpEntry>,
}

enum CpEntry {
    Utf8(String),
    Class(u16),
    NameAndType(u16, u16),
    Module(u16),
    Package(u16),
}

impl ClassBuilder {
    fn new() -> Self {
        Self { cp: Vec::new() }
    }

    fn push_cp(&mut self, entry: CpEntry) -> u16 {
        self.cp.push(entry);
        u16::try_from(self.cp.len()).unwrap()
    }

    fn utf8(&mut self, s: &str) -> u16 {
        self.push_cp(CpEntry::Utf8(s.to_string()))
    }

    fn class(&mut self, internal_name: &str) -> u16 {
        let name_idx = self.utf8(internal_name);
        self.push_cp(CpEntry::Class(name_idx))
    }

    fn module_cp(&mut self, name: &str) -> u16 {
        let name_idx = self.utf8(name);
        self.push_cp(CpEntry::Module(name_idx))
    }

    fn package_cp(&mut self, internal_pkg: &str) -> u16 {
        let name_idx = self.utf8(internal_pkg);
        self.push_cp(CpEntry::Package(name_idx))
    }

    fn name_and_type(&mut self, name: &str, desc: &str) -> u16 {
        let n = self.utf8(name);
        let d = self.utf8(desc);
        self.push_cp(CpEntry::NameAndType(n, d))
    }

    fn finish_cp(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&u16::try_from(self.cp.len() + 1).unwrap().to_be_bytes());
        for entry in &self.cp {
            match entry {
                CpEntry::Utf8(s) => {
                    out.push(1);
                    let bytes = s.as_bytes();
                    out.extend_from_slice(&u16::try_from(bytes.len()).unwrap().to_be_bytes());
                    out.extend_from_slice(bytes);
                }
                CpEntry::Class(idx) => {
                    out.push(7);
                    out.extend_from_slice(&idx.to_be_bytes());
                }
                CpEntry::NameAndType(n, d) => {
                    out.push(12);
                    out.extend_from_slice(&n.to_be_bytes());
                    out.extend_from_slice(&d.to_be_bytes());
                }
                CpEntry::Module(idx) => {
                    out.push(19);
                    out.extend_from_slice(&idx.to_be_bytes());
                }
                CpEntry::Package(idx) => {
                    out.push(20);
                    out.extend_from_slice(&idx.to_be_bytes());
                }
            }
        }
    }
}

fn build_simple_class_bytes(
    major_version: u16,
    access_flags: u16,
    internal_name: &str,
    super_internal_name: &str,
    methods: &[(&str, &str, u16)],
    fields: &[(&str, &str, u16)],
) -> Vec<u8> {
    let mut b = ClassBuilder::new();
    let this_cls = b.class(internal_name);
    let super_cls = b.class(super_internal_name);

    let mut encoded_fields = Vec::with_capacity(fields.len());
    for &(name, desc, flags) in fields {
        let n = b.utf8(name);
        let d = b.utf8(desc);
        encoded_fields.push((flags, n, d));
    }

    let mut encoded_methods = Vec::with_capacity(methods.len());
    for &(name, desc, flags) in methods {
        let n = b.utf8(name);
        let d = b.utf8(desc);
        encoded_methods.push((flags, n, d));
    }

    let mut out = Vec::new();
    out.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE]);
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&major_version.to_be_bytes());
    b.finish_cp(&mut out);

    out.extend_from_slice(&access_flags.to_be_bytes());
    out.extend_from_slice(&this_cls.to_be_bytes());
    out.extend_from_slice(&super_cls.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // interfaces_count

    out.extend_from_slice(&u16::try_from(encoded_fields.len()).unwrap().to_be_bytes());
    for (flags, n, d) in encoded_fields {
        out.extend_from_slice(&flags.to_be_bytes());
        out.extend_from_slice(&n.to_be_bytes());
        out.extend_from_slice(&d.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // attributes_count
    }

    out.extend_from_slice(&u16::try_from(encoded_methods.len()).unwrap().to_be_bytes());
    for (flags, n, d) in encoded_methods {
        out.extend_from_slice(&flags.to_be_bytes());
        out.extend_from_slice(&n.to_be_bytes());
        out.extend_from_slice(&d.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // attributes_count
    }

    out.extend_from_slice(&0u16.to_be_bytes()); // class attributes_count
    let _ = b.name_and_type("unused", "()V");
    out
}

fn build_module_info_bytes(
    module_name: &str,
    exports: &[&str],
    requires_transitive: &[&str],
) -> Vec<u8> {
    let mut b = ClassBuilder::new();
    let this_cls = b.class("module-info");
    let attr_name = b.utf8("Module");
    let mod_idx = b.module_cp(module_name);

    let req_indices: Vec<u16> = requires_transitive
        .iter()
        .map(|r| b.module_cp(r))
        .collect();
    let exp_indices: Vec<u16> = exports.iter().map(|e| b.package_cp(e)).collect();

    let mut mod_attr = Vec::new();
    mod_attr.extend_from_slice(&mod_idx.to_be_bytes());
    mod_attr.extend_from_slice(&0u16.to_be_bytes()); // module_flags
    mod_attr.extend_from_slice(&0u16.to_be_bytes()); // module_version_index

    mod_attr.extend_from_slice(&u16::try_from(req_indices.len()).unwrap().to_be_bytes());
    for r_idx in req_indices {
        mod_attr.extend_from_slice(&r_idx.to_be_bytes());
        mod_attr.extend_from_slice(&0x0020u16.to_be_bytes()); // ACC_TRANSITIVE
        mod_attr.extend_from_slice(&0u16.to_be_bytes());
    }

    mod_attr.extend_from_slice(&u16::try_from(exp_indices.len()).unwrap().to_be_bytes());
    for e_idx in exp_indices {
        mod_attr.extend_from_slice(&e_idx.to_be_bytes());
        mod_attr.extend_from_slice(&0u16.to_be_bytes());
        mod_attr.extend_from_slice(&0u16.to_be_bytes());
    }

    mod_attr.extend_from_slice(&0u16.to_be_bytes());
    mod_attr.extend_from_slice(&0u16.to_be_bytes());
    mod_attr.extend_from_slice(&0u16.to_be_bytes());

    let mut out = Vec::new();
    out.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE]);
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&55u16.to_be_bytes());
    b.finish_cp(&mut out);

    out.extend_from_slice(&0x8000u16.to_be_bytes());
    out.extend_from_slice(&this_cls.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&attr_name.to_be_bytes());
    out.extend_from_slice(&u32::try_from(mod_attr.len()).unwrap().to_be_bytes());
    out.extend_from_slice(&mod_attr);
    out
}

fn write_zip_file(path: &Path, entries: &[(&str, &[u8])]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let file = fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    for &(name, bytes) in entries {
        writer.start_file(name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
}

fn write_jmod_file(path: &Path, entries: &[(&str, &[u8])]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default();
        for &(name, bytes) in entries {
            writer.start_file(name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }
    let mut jmod_bytes = JMOD_MAGIC.to_vec();
    jmod_bytes.extend_from_slice(&cursor.into_inner());
    fs::write(path, jmod_bytes).unwrap();
}

fn probe_with_deps(
    dir: &TestDir,
    jdk: Option<PathBuf>,
    release: Release,
    deps: Vec<ModuleDependency>,
) -> WorkspaceProbe {
    dir.write("src/Main.java", "class Main {}\n");
    let mut probe = probe_workspace(dir.path());
    probe.resolved_jdk = jdk;
    probe.resolved_level = LanguageLevel::new(release, false);
    probe.layout.modules[0].dependencies = deps;
    probe
}

#[test]
fn jdk_version_key_parses_release_file_and_falls_back_to_dir_name() {
    let dir = TestDir::new();
    let jdk_with_release = dir.path().join("temurin-25");
    fs::create_dir_all(&jdk_with_release).unwrap();
    fs::write(
        jdk_with_release.join("release"),
        "IMPLEMENTOR=\"Eclipse Adoptium\"\nJAVA_VERSION=\"25.0.1\"\n",
    )
    .unwrap();
    assert_eq!(jdk_version_key(&jdk_with_release), "jdk-25.0.1");

    let jdk_without_release = dir.path().join("custom jdk@21");
    fs::create_dir_all(&jdk_without_release).unwrap();
    assert_eq!(jdk_version_key(&jdk_without_release), "custom_jdk_21");
}

#[test]
fn load_classpath_indexes_jmods_filters_unexported_and_handles_corrupt_entries() {
    let dir = TestDir::new();
    let jdk_home = dir.path().join("fake-jdk");
    let base_mod = build_module_info_bytes("java.base", &["java/lang"], &[]);
    let pub_str = build_simple_class_bytes(
        65,
        0x0001,
        "java/lang/FakeString",
        "java/lang/Object",
        &[("length", "()I", 0x0001)],
        &[],
    );
    let unexported = build_simple_class_bytes(
        65,
        0x0001,
        "sun/nio/Secret",
        "java/lang/Object",
        &[],
        &[],
    );
    let internal = build_simple_class_bytes(
        65,
        0x0001,
        "jdk/internal/misc/Unsafe",
        "java/lang/Object",
        &[],
        &[],
    );

    write_jmod_file(
        &jdk_home.join("jmods/java.base.jmod"),
        &[
            ("classes/module-info.class", &base_mod),
            ("classes/java/lang/FakeString.class", &pub_str),
            ("classes/java/lang/Corrupt.class", b"not-a-classfile"),
            ("classes/sun/nio/Secret.class", &unexported),
            ("classes/jdk/internal/misc/Unsafe.class", &internal),
        ],
    );
    fs::write(jdk_home.join("jmods/corrupt.jmod"), b"bad-magic").unwrap();

    let probe = probe_with_deps(&dir, Some(jdk_home), Release::JAVA_21, vec![]);
    let (index, _) = load_classpath(&probe);

    assert!(index.modules.contains_key("java.base"));
    assert!(index.type_by_fqn("java.lang.FakeString").is_some());
    assert!(index.type_by_fqn("sun.nio.Secret").is_none());
    assert!(index.type_by_fqn("jdk.internal.misc.Unsafe").is_none());
}

#[test]
fn load_classpath_indexes_jdk8_rt_jar_skipping_internal_packages() {
    let dir = TestDir::new();
    let jdk_home = dir.path().join("jdk8");
    let obj =
        build_simple_class_bytes(52, 0x0001, "java/lang/FakeObj", "java/lang/Object", &[], &[]);
    let sun_cls =
        build_simple_class_bytes(52, 0x0001, "sun/misc/Hidden", "java/lang/Object", &[], &[]);
    let com_sun_cls =
        build_simple_class_bytes(52, 0x0001, "com/sun/Hidden2", "java/lang/Object", &[], &[]);

    write_zip_file(
        &jdk_home.join("jre/lib/rt.jar"),
        &[
            ("java/lang/FakeObj.class", &obj),
            ("sun/misc/Hidden.class", &sun_cls),
            ("com/sun/Hidden2.class", &com_sun_cls),
        ],
    );

    let probe = probe_with_deps(&dir, Some(jdk_home), Release::JAVA_8, vec![]);
    let (index, _) = load_classpath(&probe);

    let ty = index
        .type_by_fqn("java.lang.FakeObj")
        .expect("FakeObj indexed");
    assert_eq!(ty.origin, BinaryOrigin::Jdk { module_name: None });
    assert!(index.type_by_fqn("sun.misc.Hidden").is_none());
    assert!(index.type_by_fqn("com.sun.Hidden2").is_none());
}

#[test]
fn load_classpath_indexes_mrjar_class_dir_and_discovers_companions() {
    let dir = TestDir::new();
    let base_cls = build_simple_class_bytes(
        52,
        0x0001,
        "com/acme/Widget",
        "java/lang/Object",
        &[("legacy", "()V", 0x0001)],
        &[],
    );
    let v17_cls = build_simple_class_bytes(
        61,
        0x0001,
        "com/acme/Widget",
        "java/lang/Object",
        &[("modern", "()I", 0x0001)],
        &[],
    );
    let v25_cls = build_simple_class_bytes(
        69,
        0x0001,
        "com/acme/Widget",
        "java/lang/Object",
        &[("future", "()V", 0x0001)],
        &[],
    );

    let jar_path = dir.path().join("libs/acme-1.0.jar");
    write_zip_file(
        &jar_path,
        &[
            (
                "META-INF/MANIFEST.MF",
                b"Manifest-Version: 1.0\r\nMulti-Release: true\r\n",
            ),
            ("com/acme/Widget.class", &base_cls),
            ("META-INF/versions/17/com/acme/Widget.class", &v17_cls),
            ("META-INF/versions/25/com/acme/Widget.class", &v25_cls),
        ],
    );
    let sources_jar = dir.path().join("libs/acme-1.0-sources.jar");
    write_zip_file(
        &sources_jar,
        &[(
            "com/acme/Widget.java",
            b"package com.acme;\n/** Widget doc. */\npublic class Widget { public int modern() { return 1; } }\n",
        )],
    );
    let javadoc_jar = dir.path().join("libs/acme-1.0-javadoc.jar");
    write_zip_file(&javadoc_jar, &[]);

    let dir_cls = build_simple_class_bytes(
        61,
        0x0001,
        "com/local/Helper",
        "java/lang/Object",
        &[("ping", "()V", 0x0001)],
        &[],
    );
    let classes_dir = dir.path().join("build/classes");
    fs::create_dir_all(classes_dir.join("com/local")).unwrap();
    fs::write(classes_dir.join("com/local/Helper.class"), &dir_cls).unwrap();

    let probe = probe_with_deps(
        &dir,
        None,
        Release::JAVA_21,
        vec![
            ModuleDependency::Path {
                path: "libs/acme-1.0.jar".to_string(),
                scope: RootScope::Main,
            },
            ModuleDependency::Path {
                path: "build/classes".to_string(),
                scope: RootScope::Main,
            },
        ],
    );

    let (index, companions) = load_classpath(&probe);
    let widget = index
        .type_by_fqn("com.acme.Widget")
        .expect("Widget indexed");
    assert!(widget.members.iter().any(|m| m.name == "modern"));
    assert!(!widget.members.iter().any(|m| m.name == "legacy"));
    assert!(!widget.members.iter().any(|m| m.name == "future"));
    assert_eq!(
        widget.origin,
        BinaryOrigin::Jar {
            label: "libs/acme-1.0.jar".to_string()
        }
    );

    let comp = companions
        .get("libs/acme-1.0.jar")
        .expect("companions discovered");
    assert_eq!(comp.sources_jar.as_deref(), Some(sources_jar.as_path()));
    assert_eq!(comp.javadoc_jar.as_deref(), Some(javadoc_jar.as_path()));

    let helper = index
        .type_by_fqn("com.local.Helper")
        .expect("Helper from class dir");
    assert!(helper.members.iter().any(|m| m.name == "ping"));
}

#[test]
fn materialize_binary_location_prefers_sources_and_falls_back_to_stub() {
    let dir = TestDir::new();
    let cache = dir.path().join("cache");

    // 1. JAR with companion sources.jar
    let jar_cls = build_simple_class_bytes(
        61,
        0x0001,
        "com/acme/Greeter",
        "java/lang/Object",
        &[("greet", "()V", 0x0001)],
        &[],
    );
    let jar_path = dir.path().join("libs/greeter.jar");
    write_zip_file(&jar_path, &[("com/acme/Greeter.class", &jar_cls)]);
    let sources_path = dir.path().join("libs/greeter-sources.jar");
    let greeter_src = "package com.acme;\npublic class Greeter {\n    public void greet() {}\n}\n";
    write_zip_file(&sources_path, &[("com/acme/Greeter.java", greeter_src.as_bytes())]);

    // 2. JAR without companion sources -> stub fallback
    let stub_cls = build_simple_class_bytes(
        61,
        0x0001,
        "com/acme/StubOnly",
        "java/lang/Object",
        &[("run", "()I", 0x0001)],
        &[],
    );
    let stub_jar = dir.path().join("libs/stubonly.jar");
    write_zip_file(&stub_jar, &[("com/acme/StubOnly.class", &stub_cls)]);

    // 3. JDK with src.zip
    let jdk_home = dir.path().join("jdk-21");
    let jdk_mod = build_module_info_bytes("java.base", &["java/lang"], &[]);
    let jdk_cls = build_simple_class_bytes(
        65,
        0x0001,
        "java/lang/FakeJdkType",
        "java/lang/Object",
        &[("value", "()I", 0x0001)],
        &[],
    );
    write_jmod_file(
        &jdk_home.join("jmods/java.base.jmod"),
        &[
            ("classes/module-info.class", &jdk_mod),
            ("classes/java/lang/FakeJdkType.class", &jdk_cls),
        ],
    );
    fs::write(jdk_home.join("release"), "JAVA_VERSION=\"21.0.2\"\n").unwrap();
    let jdk_src_text = "package java.lang;\npublic class FakeJdkType {\n    public int value() { return 0; }\n}\n";
    write_zip_file(
        &jdk_home.join("lib/src.zip"),
        &[("java.base/java/lang/FakeJdkType.java", jdk_src_text.as_bytes())],
    );

    let probe = probe_with_deps(
        &dir,
        Some(jdk_home.clone()),
        Release::JAVA_21,
        vec![
            ModuleDependency::Path {
                path: "libs/greeter.jar".to_string(),
                scope: RootScope::Main,
            },
            ModuleDependency::Path {
                path: "libs/stubonly.jar".to_string(),
                scope: RootScope::Main,
            },
        ],
    );
    let (index, companions) = load_classpath(&probe);

    let greeter_target = BinaryTarget {
        origin: BinaryOrigin::Jar {
            label: "libs/greeter.jar".to_string(),
        },
        internal_name: "com/acme/Greeter".to_string(),
        qualified_name: "com.acme.Greeter".to_string(),
        member_name: Some(SmolStr::new("greet")),
    };
    let (path, src, range) = materialize_binary_location_in_cache_root(
        &cache,
        &index,
        Some(&jdk_home),
        &companions,
        &greeter_target,
    )
    .expect("materialized from sources.jar");
    assert!(path.starts_with(cache.join("jar-src")));
    assert!(path.is_file());
    assert_eq!(src, greeter_src);
    let range = range.expect("greet range");
    assert_eq!(
        &src[usize::from(range.start())..usize::from(range.end())],
        "greet"
    );

    let stub_target = BinaryTarget {
        origin: BinaryOrigin::Jar {
            label: "libs/stubonly.jar".to_string(),
        },
        internal_name: "com/acme/StubOnly".to_string(),
        qualified_name: "com.acme.StubOnly".to_string(),
        member_name: Some(SmolStr::new("run")),
    };
    let (stub_path, stub_src, stub_range) = materialize_binary_location_in_cache_root(
        &cache,
        &index,
        Some(&jdk_home),
        &companions,
        &stub_target,
    )
    .expect("materialized stub fallback");
    assert!(stub_path.starts_with(cache.join("stubs")));
    assert!(stub_path.is_file());
    assert!(stub_src.contains("public int run()"));
    let stub_range = stub_range.expect("run range");
    assert_eq!(
        &stub_src[usize::from(stub_range.start())..usize::from(stub_range.end())],
        "run"
    );

    let jdk_target = BinaryTarget {
        origin: BinaryOrigin::Jdk {
            module_name: Some(SmolStr::new("java.base")),
        },
        internal_name: "java/lang/FakeJdkType".to_string(),
        qualified_name: "java.lang.FakeJdkType".to_string(),
        member_name: Some(SmolStr::new("value")),
    };
    let (jdk_path, jdk_src, jdk_range) = materialize_binary_location_in_cache_root(
        &cache,
        &index,
        Some(&jdk_home),
        &companions,
        &jdk_target,
    )
    .expect("materialized from jdk src.zip");
    assert!(jdk_path.starts_with(cache.join("jdk-src/jdk-21.0.2")));
    assert_eq!(jdk_src, jdk_src_text);
    let jdk_range = jdk_range.expect("value range");
    assert_eq!(
        &jdk_src[usize::from(jdk_range.start())..usize::from(jdk_range.end())],
        "value"
    );

    // Removing src.zip still reuses cached file on disk.
    fs::remove_file(jdk_home.join("lib/src.zip")).unwrap();
    let (reused_path, reused_src, _) = materialize_binary_location_in_cache_root(
        &cache,
        &index,
        Some(&jdk_home),
        &companions,
        &jdk_target,
    )
    .expect("reused cached jdk source");
    assert_eq!(reused_path, jdk_path);
    assert_eq!(reused_src, jdk_src_text);
}

#[test]
fn lookup_binary_javadoc_uses_sources_jar_and_falls_back_to_javadoc_jar_html() {
    let dir = TestDir::new();
    let src_jar = dir.path().join("libs/doclib-sources.jar");
    write_zip_file(
        &src_jar,
        &[(
            "com/acme/FromSrc.java",
            b"package com.acme;\n/** Source javadoc for {@code FromSrc}. */\npublic class FromSrc {}\n",
        )],
    );

    let html_jar = dir.path().join("libs/htmlonly-javadoc.jar");
    write_zip_file(
        &html_jar,
        &[(
            "com/acme/FromHtml.html",
            br#"<html><body>
<section class="class-description" id="class-description">
<div class="block">HTML javadoc for <code>FromHtml</code>.</div>
</section>
<section class="detail" id="compute()">
<h3>compute</h3>
<div class="block">Computes a result.</div>
</section>
</body></html>"#,
        )],
    );

    let mut companions = JarCompanions::default();
    companions.insert(
        "libs/doclib.jar",
        JarCompanionPaths {
            sources_jar: Some(src_jar),
            javadoc_jar: None,
        },
    );
    companions.insert(
        "libs/htmlonly.jar",
        JarCompanionPaths {
            sources_jar: None,
            javadoc_jar: Some(html_jar),
        },
    );

    let src_target = BinaryTarget {
        origin: BinaryOrigin::Jar {
            label: "libs/doclib.jar".to_string(),
        },
        internal_name: "com/acme/FromSrc".to_string(),
        qualified_name: "com.acme.FromSrc".to_string(),
        member_name: None,
    };
    assert_eq!(
        lookup_binary_javadoc(None, &companions, &src_target).as_deref(),
        Some("Source javadoc for `FromSrc`.")
    );

    let html_target = BinaryTarget {
        origin: BinaryOrigin::Jar {
            label: "libs/htmlonly.jar".to_string(),
        },
        internal_name: "com/acme/FromHtml".to_string(),
        qualified_name: "com.acme.FromHtml".to_string(),
        member_name: Some(SmolStr::new("compute")),
    };
    assert_eq!(
        lookup_binary_javadoc(None, &companions, &html_target).as_deref(),
        Some("Computes a result.")
    );
}

fn write_jimage_file(path: &Path, entries: &[(&str, &str, &str, &str, &[u8])]) {
    write_jimage_file_with_options(path, 0, false, false, entries);
}

fn write_jimage_file_with_options(
    path: &Path,
    minor_version: u16,
    little_endian: bool,
    include_v1_1_attr_and_compressed: bool,
    entries: &[(&str, &str, &str, &str, &[u8])],
) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let mut strings = vec![0u8];
    let mut intern = |s: &str| -> usize {
        if s.is_empty() {
            return 0;
        }
        let off = strings.len();
        strings.extend_from_slice(s.as_bytes());
        strings.push(0);
        off
    };

    let mut attr_offsets: Vec<i32> = Vec::with_capacity(entries.len());
    let mut attr_data = Vec::new();
    let mut data_section = Vec::new();

    let push_attr = |buf: &mut Vec<u8>, kind: u8, val: usize| {
        let be = (val as u64).to_be_bytes();
        let first_nonzero = be.iter().position(|&b| b != 0).unwrap_or(7);
        let used = &be[first_nonzero..];
        let len_code = u8::try_from(used.len() - 1).unwrap();
        buf.push((kind << 3) | len_code);
        buf.extend_from_slice(used);
    };

    for (idx, &(module, parent_pkg, base, ext, payload)) in entries.iter().enumerate() {
        let mod_off = intern(module);
        let par_off = intern(parent_pkg);
        let base_off = intern(base);
        let ext_off = intern(ext);
        let data_off = data_section.len();
        data_section.extend_from_slice(payload);

        attr_offsets.push(i32::try_from(attr_data.len()).unwrap());
        if mod_off != 0 {
            push_attr(&mut attr_data, 1, mod_off);
        }
        if par_off != 0 {
            push_attr(&mut attr_data, 2, par_off);
        }
        if base_off != 0 {
            push_attr(&mut attr_data, 3, base_off);
        }
        if ext_off != 0 {
            push_attr(&mut attr_data, 4, ext_off);
        }
        if include_v1_1_attr_and_compressed {
            // Attribute kind 8 (e.g. preview flags in v1.1) should be skipped transparently.
            push_attr(&mut attr_data, 8, 1);
            if idx + 1 == entries.len() {
                // Mark last entry as compressed so reader skips it cleanly.
                push_attr(&mut attr_data, 6, payload.len().max(1));
            }
        }
        push_attr(&mut attr_data, 5, data_off);
        push_attr(&mut attr_data, 7, payload.len());
        attr_data.push(0);
    }

    let encode_u32 = |v: u32| -> [u8; 4] {
        if little_endian {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let encode_i32 = |v: i32| -> [u8; 4] {
        if little_endian {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };

    let count = u32::try_from(entries.len()).unwrap();
    let version = 0x0001_0000u32 | u32::from(minor_version);
    let mut out = Vec::new();
    out.extend_from_slice(&encode_u32(0xCAFE_DADA));
    out.extend_from_slice(&encode_u32(version));
    out.extend_from_slice(&encode_u32(0)); // flags
    out.extend_from_slice(&encode_u32(count)); // resource_count
    out.extend_from_slice(&encode_u32(count)); // table_length
    out.extend_from_slice(&encode_u32(u32::try_from(attr_data.len()).unwrap()));
    out.extend_from_slice(&encode_u32(u32::try_from(strings.len()).unwrap()));
    for i in 0..entries.len() {
        let redirect = -1 - i32::try_from(i).unwrap();
        out.extend_from_slice(&encode_i32(redirect));
    }
    for off in attr_offsets {
        out.extend_from_slice(&encode_i32(off));
    }
    out.extend_from_slice(&attr_data);
    out.extend_from_slice(&strings);
    out.extend_from_slice(&data_section);
    fs::write(path, out).unwrap();
}

#[test]
fn load_classpath_indexes_jimage_v1_1_little_endian_with_unknown_attrs_and_compressed() {
    let dir = TestDir::new();
    let jdk_home = dir.path().join("fake-jimage-v11-jdk");
    let base_mod = build_module_info_bytes("java.base", &["java/lang"], &[]);
    let str_cls = build_simple_class_bytes(
        69,
        0x0001,
        "java/lang/PreviewString",
        "java/lang/Object",
        &[("isEmpty", "()Z", 0x0001)],
        &[],
    );
    let skipped_cls = build_simple_class_bytes(
        69,
        0x0001,
        "java/lang/CompressedType",
        "java/lang/Object",
        &[],
        &[],
    );

    write_jimage_file_with_options(
        &jdk_home.join("lib/modules"),
        1,
        true,
        true,
        &[
            ("java.base", "", "module-info", "class", &base_mod),
            ("java.base", "java/lang", "PreviewString", "class", &str_cls),
            (
                "java.base",
                "java/lang",
                "CompressedType",
                "class",
                &skipped_cls,
            ),
        ],
    );

    let probe = probe_with_deps(&dir, Some(jdk_home), Release::JAVA_25, vec![]);
    let (index, _) = load_classpath(&probe);

    assert!(index.modules.contains_key("java.base"));
    let ty = index
        .type_by_fqn("java.lang.PreviewString")
        .expect("PreviewString indexed from v1.1 LE jimage");
    assert!(ty.members.iter().any(|m| m.name == "isEmpty"));
    assert!(index.type_by_fqn("java.lang.CompressedType").is_none());
}

#[test]
fn load_classpath_indexes_jimage_modules_and_filters_entries() {
    let dir = TestDir::new();
    let jdk_home = dir.path().join("fake-jimage-jdk");
    let base_mod = build_module_info_bytes("java.base", &["java/lang", "java/util"], &[]);
    let str_cls = build_simple_class_bytes(
        65,
        0x0001,
        "java/lang/FakeString",
        "java/lang/Object",
        &[("length", "()I", 0x0001)],
        &[],
    );
    let entry_cls = build_simple_class_bytes(
        65,
        0x0001,
        "java/util/Map$Entry",
        "java/lang/Object",
        &[],
        &[],
    );
    let anon_cls = build_simple_class_bytes(
        65,
        0x0001,
        "java/lang/FakeString$1",
        "java/lang/Object",
        &[],
        &[],
    );
    let pkg_info = build_simple_class_bytes(
        65,
        0x1600,
        "java/lang/package-info",
        "java/lang/Object",
        &[],
        &[],
    );
    let unexported = build_simple_class_bytes(
        65,
        0x0001,
        "sun/nio/Secret",
        "java/lang/Object",
        &[],
        &[],
    );
    let internal = build_simple_class_bytes(
        65,
        0x0001,
        "jdk/internal/misc/Unsafe",
        "java/lang/Object",
        &[],
        &[],
    );

    write_jimage_file(
        &jdk_home.join("lib/modules"),
        &[
            ("packages", "java.lang", "java.base", "", b""),
            ("modules", "java.base", "java.lang", "", b""),
            ("java.base", "", "module-info", "class", &base_mod),
            ("java.base", "java/lang", "FakeString", "class", &str_cls),
            ("java.base", "java/util", "Map$Entry", "class", &entry_cls),
            ("java.base", "java/lang", "FakeString$1", "class", &anon_cls),
            ("java.base", "java/lang", "package-info", "class", &pkg_info),
            ("java.base", "java/lang", "Corrupt", "class", b"not-a-classfile"),
            ("java.base", "sun/nio", "Secret", "class", &unexported),
            ("java.base", "jdk/internal/misc", "Unsafe", "class", &internal),
        ],
    );

    let probe = probe_with_deps(&dir, Some(jdk_home), Release::JAVA_21, vec![]);
    let (index, _) = load_classpath(&probe);

    let base = index.modules.get("java.base").expect("java.base module");
    assert!(base.exports.iter().any(|p| p == "java.lang"));
    assert!(base.exports.iter().any(|p| p == "java.util"));

    let fake_str = index
        .type_by_fqn("java.lang.FakeString")
        .expect("FakeString indexed");
    assert_eq!(
        fake_str.origin,
        BinaryOrigin::Jdk {
            module_name: Some(SmolStr::new("java.base"))
        }
    );
    assert!(fake_str.members.iter().any(|m| m.name == "length"));
    assert!(index.type_by_fqn("java.util.Map$Entry").is_some());
    assert!(index.type_by_fqn("java.lang.FakeString$1").is_none());
    assert!(index.type_by_fqn("java.lang.package-info").is_none());
    assert!(index.type_by_fqn("sun.nio.Secret").is_none());
    assert!(index.type_by_fqn("jdk.internal.misc.Unsafe").is_none());
}

#[test]
fn load_classpath_handles_corrupt_jimage_modules_gracefully() {
    let dir = TestDir::new();
    let jdk_home = dir.path().join("corrupt-jimage-jdk");
    fs::create_dir_all(jdk_home.join("lib")).unwrap();
    fs::write(jdk_home.join("lib/modules"), b"not-a-jimage").unwrap();

    let probe = probe_with_deps(&dir, Some(jdk_home), Release::JAVA_21, vec![]);
    let (index, _) = load_classpath(&probe);
    assert_eq!(index.type_count(), 0);
    assert_eq!(index.module_count(), 0);
}

#[test]
fn ct_sym_version_char_encodes_base36_uppercase() {
    assert_eq!(ct_sym_version_char(Release::JAVA_8), Some('8'));
    assert_eq!(ct_sym_version_char(Release::JAVA_9), Some('9'));
    assert_eq!(ct_sym_version_char(Release::JAVA_10), Some('A'));
    assert_eq!(ct_sym_version_char(Release::JAVA_17), Some('H'));
    assert_eq!(ct_sym_version_char(Release::JAVA_21), Some('L'));
    assert_eq!(ct_sym_version_char(Release::JAVA_26), Some('Q'));
    assert_eq!(ct_sym_version_char(Release::JAVA_27), Some('R'));
}

#[test]
fn load_classpath_uses_ct_sym_for_historical_release_and_falls_back_when_absent() {
    let dir = TestDir::new();
    let jdk_home = dir.path().join("jdk-21");
    fs::create_dir_all(&jdk_home).unwrap();
    fs::write(jdk_home.join("release"), "JAVA_VERSION=\"21.0.2\"\n").unwrap();

    // Fallback JDK 21 jimage contains `FutureOnly` (not in ct.sym for 8/11/17).
    let jimage_mod = build_module_info_bytes("java.base", &["java/lang"], &[]);
    let jimage_future = build_simple_class_bytes(
        65,
        0x0001,
        "java/lang/FutureOnly",
        "java/lang/Object",
        &[],
        &[],
    );
    write_jimage_file(
        &jdk_home.join("lib/modules"),
        &[
            ("java.base", "", "module-info", "class", &jimage_mod),
            ("java.base", "java/lang", "FutureOnly", "class", &jimage_future),
        ],
    );

    let str_v8 = build_simple_class_bytes(
        52,
        0x0001,
        "java/lang/FakeString",
        "java/lang/Object",
        &[("length", "()I", 0x0001)],
        &[],
    );
    let str_v11 = build_simple_class_bytes(
        55,
        0x0001,
        "java/lang/FakeString",
        "java/lang/Object",
        &[("length", "()I", 0x0001), ("isBlank", "()Z", 0x0001)],
        &[],
    );
    let obj_shared = build_simple_class_bytes(
        52,
        0x0001,
        "java/lang/FakeObject",
        "java/lang/Object",
        &[],
        &[],
    );
    let sun_cls = build_simple_class_bytes(
        55,
        0x0001,
        "sun/nio/Hidden",
        "java/lang/Object",
        &[],
        &[],
    );
    let mod_info_9b = build_module_info_bytes("java.base", &["java/lang"], &[]);

    write_zip_file(
        &jdk_home.join("lib/ct.sym"),
        &[
            ("89A/java.base/java/lang/FakeString.sig", &str_v8),
            ("BC/java.base/java/lang/FakeString.sig", &str_v11),
            ("89ABC/java.base/java/lang/FakeObject.sig", &obj_shared),
            ("BC/java.base/sun/nio/Hidden.sig", &sun_cls),
            ("9ABC/java.base/module-info.sig", &mod_info_9b),
            ("B-modules/java.base/system-modules", b"ignored"),
            ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\n"),
        ],
    );

    // Target Java 8 -> matches 89A + 89ABC, module_name = None, no isBlank.
    let probe_8 = probe_with_deps(&dir, Some(jdk_home.clone()), Release::JAVA_8, vec![]);
    let (idx_8, _) = load_classpath(&probe_8);
    let s8 = idx_8
        .type_by_fqn("java.lang.FakeString")
        .expect("FakeString at Java 8");
    assert_eq!(s8.origin, BinaryOrigin::Jdk { module_name: None });
    assert!(s8.members.iter().any(|m| m.name == "length"));
    assert!(!s8.members.iter().any(|m| m.name == "isBlank"));
    assert!(idx_8.type_by_fqn("java.lang.FakeObject").is_some());
    assert!(idx_8.type_by_fqn("java.lang.FutureOnly").is_none());
    assert_eq!(idx_8.module_count(), 0);

    // Target Java 11 ('B') -> matches BC + 89ABC + 9ABC module-info, has isBlank & java.base.
    let probe_11 = probe_with_deps(&dir, Some(jdk_home.clone()), Release::JAVA_11, vec![]);
    let (idx_11, _) = load_classpath(&probe_11);
    let s11 = idx_11
        .type_by_fqn("java.lang.FakeString")
        .expect("FakeString at Java 11");
    assert_eq!(
        s11.origin,
        BinaryOrigin::Jdk {
            module_name: Some(SmolStr::new("java.base"))
        }
    );
    assert!(s11.members.iter().any(|m| m.name == "isBlank"));
    assert!(idx_11.type_by_fqn("sun.nio.Hidden").is_none());
    assert!(idx_11.type_by_fqn("java.lang.FutureOnly").is_none());
    let base_11 = idx_11.modules.get("java.base").expect("java.base module");
    assert!(base_11.exports.iter().any(|p| p == "java.lang"));

    // Target Java 17 ('H') is absent from synthetic ct.sym -> falls back to lib/modules.
    let probe_17 = probe_with_deps(&dir, Some(jdk_home), Release::JAVA_17, vec![]);
    let (idx_17, _) = load_classpath(&probe_17);
    assert!(idx_17.type_by_fqn("java.lang.FutureOnly").is_some());
    assert!(idx_17.type_by_fqn("java.lang.FakeString").is_none());
}

#[test]
fn real_local_jdk_ct_sym_smoke_test_if_installed() {
    let jdks = discover_jdks(None);
    let Some(jdk) = jdks.into_iter().find(|j| {
        j.home.join("lib/ct.sym").is_file()
            && probe_jdk_release(&j.home).is_some_and(|r| r >= Release::JAVA_18)
    }) else {
        return;
    };

    let dir = TestDir::new();
    let probe_8 = probe_with_deps(&dir, Some(jdk.home.clone()), Release::JAVA_8, vec![]);
    let (idx_8, _) = load_classpath(&probe_8);
    eprintln!(
        "real_local_jdk_ct_sym_smoke_test ({}, JAVA_8): types={}, modules={}",
        jdk.home.display(),
        idx_8.type_count(),
        idx_8.module_count()
    );
    let str_8 = idx_8
        .type_by_fqn("java.lang.String")
        .expect("expected java.lang.String in ct.sym at JAVA_8");
    assert!(str_8.members.iter().any(|m| m.name == "length"));
    assert!(
        !str_8.members.iter().any(|m| m.name == "isBlank"),
        "java.lang.String#isBlank should not exist at JAVA_8"
    );
    assert!(
        idx_8.type_by_fqn("java.lang.Record").is_none(),
        "java.lang.Record should not exist at JAVA_8"
    );

    let probe_17 = probe_with_deps(&dir, Some(jdk.home.clone()), Release::JAVA_17, vec![]);
    let (idx_17, _) = load_classpath(&probe_17);
    eprintln!(
        "real_local_jdk_ct_sym_smoke_test ({}, JAVA_17): types={}, modules={}",
        jdk.home.display(),
        idx_17.type_count(),
        idx_17.module_count()
    );
    let str_17 = idx_17
        .type_by_fqn("java.lang.String")
        .expect("expected java.lang.String in ct.sym at JAVA_17");
    assert!(
        str_17.members.iter().any(|m| m.name == "isBlank"),
        "expected java.lang.String#isBlank in ct.sym at JAVA_17"
    );
    assert!(
        idx_17.type_by_fqn("java.lang.Record").is_some(),
        "expected java.lang.Record in ct.sym at JAVA_17"
    );
    assert!(idx_17.modules.contains_key("java.base"));
}

#[test]
fn real_local_jdk_smoke_test_if_installed() {
    let jdks = discover_jdks(None);
    let Some(jdk) = jdks.first() else {
        return;
    };
    let has_jdk_archive = jdk.home.join("jmods").is_dir()
        || jdk.home.join("lib/modules").is_file()
        || jdk.home.join("jre/lib/rt.jar").is_file()
        || jdk.home.join("lib/rt.jar").is_file();
    if !has_jdk_archive {
        return;
    }

    let dir = TestDir::new();
    let probe = probe_with_deps(
        &dir,
        Some(jdk.home.clone()),
        jdk.release.unwrap_or(Release::JAVA_21),
        vec![],
    );
    let (index, _) = load_classpath(&probe);
    eprintln!(
        "real_local_jdk_smoke_test_if_installed ({}): types={}, modules={}",
        jdk.home.display(),
        index.type_count(),
        index.module_count()
    );

    let string_ty = index
        .type_by_fqn("java.lang.String")
        .unwrap_or_else(|| panic!("expected java.lang.String in real JDK {:?}", jdk.home));
    assert!(
        string_ty.members.iter().any(|m| m.name == "length"),
        "expected java.lang.String#length in real JDK {:?}",
        jdk.home
    );
    assert!(
        index.type_by_fqn("java.util.List").is_some(),
        "expected java.util.List in real JDK {:?}",
        jdk.home
    );
    assert!(
        index.type_by_fqn("java.util.Map.Entry").is_some(),
        "expected java.util.Map.Entry in real JDK {:?}",
        jdk.home
    );
    assert!(
        index.type_count() > 1000,
        "expected > 1000 types in real JDK {:?}, got {}",
        jdk.home,
        index.type_count()
    );
    if jdk.home.join("jmods").is_dir() || jdk.home.join("lib/modules").is_file() {
        assert!(
            index.module_count() >= 1,
            "expected >= 1 module in real JDK {:?}",
            jdk.home
        );
        let base = index
            .modules
            .get("java.base")
            .expect("expected java.base module");
        assert!(base.exports.iter().any(|p| p == "java.lang"));
        assert!(base.exports.iter().any(|p| p == "java.util"));
    }
}
