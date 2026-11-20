use std::{
    env, fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use hir::{LanguageLevel, ModuleDependency, Release, RootScope};

use super::{maven::parse_maven_properties, *};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!(
            "java_analyzer_ws_test_{}_{}_{}",
            label,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, rel: &str, content: &str) {
        let full = self.0.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(full, content).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn ant_and_ivy_pair_without_conflict_and_extract_compile_time_deps() {
    let tmp = TempDir::new("ant_ivy");
    tmp.write(
        "build.xml",
        r#"<project name="demo" default="compile" basedir=".">
                <property name="src.dir" value="src"/>
                <property name="test.dir" value="test"/>
                <property name="lib.dir" value="lib"/>
                <target name="compile">
                    <javac srcdir="${src.dir}" destdir="build/classes" release="17">
                        <classpath>
                            <fileset dir="${lib.dir}" includes="**/*.jar"/>
                        </classpath>
                    </javac>
                </target>
            </project>"#,
    );
    tmp.write(
        "ivy.xml",
        r#"<ivy-module version="2.0">
                <info organisation="com.example" module="demo"/>
                <dependencies>
                    <dependency org="com.google.guava" name="guava" rev="33.0.0-jre" conf="compile->default"/>
                    <dependency org="org.junit.jupiter" name="junit-jupiter" rev="5.10.2" conf="test->default"/>
                    <dependency org="org.postgresql" name="postgresql" rev="42.7.2" conf="runtime->default"/>
                </dependencies>
            </ivy-module>"#,
    );
    tmp.write("lib/helper.jar", "");
    tmp.write(
        "src/com/example/App.java",
        "package com.example;\npublic class App {}\n",
    );

    let probe = probe_workspace(&tmp.0);
    assert!(
        probe.conflicts.is_empty(),
        "build.xml + ivy.xml must not conflict: {:?}",
        probe.conflicts
    );
    assert_eq!(probe.resolved_build, BuildToolKind::Ant);
    assert_eq!(
        probe.resolved_level,
        LanguageLevel::new(Release::JAVA_17, false)
    );

    let root_mod = &probe.layout.modules[0];
    assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "com.google.guava".to_string(),
        artifact: "guava".to_string(),
        version: "33.0.0-jre".to_string(),
        classifier: None,
        scope: RootScope::Main,
    }));
    assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "org.junit.jupiter".to_string(),
        artifact: "junit-jupiter".to_string(),
        version: "5.10.2".to_string(),
        classifier: None,
        scope: RootScope::Test,
    }));
    // Runtime-only dependency must be excluded from compile-time LSP model
    assert!(!root_mod.dependencies.iter().any(|d| match d {
        ModuleDependency::Maven { artifact, .. } => artifact == "postgresql",
        _ => false,
    }));
    // Local JAR from <fileset dir="${lib.dir}"/>
    assert!(root_mod.dependencies.contains(&ModuleDependency::Path {
        path: "lib/helper.jar".to_string(),
        scope: RootScope::Main,
    }));
}

#[test]
fn detects_build_and_release_conflicts_and_disambiguates_via_tier1_or_reconcile() {
    let tmp = TempDir::new("conflicts");
    tmp.write(
        "pom.xml",
        r#"<project>
                <properties>
                    <maven.compiler.release>17</maven.compiler.release>
                </properties>
            </project>"#,
    );
    tmp.write(
        "build.gradle.kts",
        r#"java {
                toolchain {
                    languageVersion.set(JavaLanguageVersion.of(21))
                }
            }"#,
    );
    tmp.write(".java-version", "25\n");

    let probe = probe_workspace(&tmp.0);
    assert_eq!(probe.conflicts.len(), 2);
    assert!(probe
        .conflicts
        .iter()
        .any(|c| matches!(c, Conflict::BuildTool(_))));
    assert!(probe
        .conflicts
        .iter()
        .any(|c| matches!(c, Conflict::Release(_))));

    // Reconciling external version files updates .java-version and pom.xml to 21
    let modified = reconcile_version_conflict(&tmp.0, Release::JAVA_21);
    assert_eq!(modified.len(), 2);

    let probe_after_reconcile = probe_workspace(&tmp.0);
    assert!(
        !probe_after_reconcile
            .conflicts
            .iter()
            .any(|c| matches!(c, Conflict::Release(_))),
        "Release conflict should be resolved after reconciling external files"
    );
    assert_eq!(
        probe_after_reconcile.resolved_level,
        LanguageLevel::new(Release::JAVA_21, false)
    );

    // Now disambiguate the remaining build tool conflict with a minimal .java-analyzer.toml
    let mut cfg = ExplicitConfig::default();
    cfg.build = Some(BuildToolKind::Gradle);
    cfg.release = Some(Release::JAVA_21); // Matches uniquely inferred release -> must be omitted!
    let toml_text = render_minimal_toml(&probe_after_reconcile, &cfg);
    assert_eq!(toml_text, "build = \"gradle\"\n");

    tmp.write(".java-analyzer.toml", &toml_text);
    let final_probe = probe_workspace(&tmp.0);
    assert!(final_probe.conflicts.is_empty());
    assert_eq!(final_probe.resolved_build, BuildToolKind::Gradle);
}

#[test]
fn meta_inf_manifest_signals_resource_root_and_tier1_deltas_override() {
    let tmp = TempDir::new("meta_inf_and_deltas");
    tmp.write(
        "src/com/example/App.java",
        "package com.example;\nclass App {}\n",
    );
    tmp.write("assets/META-INF/MANIFEST.MF", "Manifest-Version: 1.0\n");
    tmp.write(
        ".java-analyzer.toml",
        r#"
[modules."."]
sources = { include = ["generated/src"] }
dependencies = { include = ["com.google.guava:guava:33.3.1-jre", "github:google/guava#v33.3.1"] }
"#,
    );

    let probe = probe_workspace(&tmp.0);
    let root_mod = &probe.layout.modules[0];
    assert_eq!(root_mod.rel_path, ".");
    assert!(root_mod.sources.iter().any(|s| s.rel_path == "src"));
    assert!(root_mod
        .sources
        .iter()
        .any(|s| s.rel_path == "generated/src"));
    assert!(
        root_mod.resources.iter().any(|r| r.rel_path == "assets"),
        "assets/ containing META-INF/MANIFEST.MF should be inferred as a ResourceRoot"
    );
    assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "com.google.guava".to_string(),
        artifact: "guava".to_string(),
        version: "33.3.1-jre".to_string(),
        classifier: None,
        scope: RootScope::Main,
    }));
    assert!(root_mod.dependencies.contains(&ModuleDependency::Git {
        repo: "github:google/guava".to_string(),
        reference: Some("v33.3.1".to_string()),
        scope: RootScope::Main,
    }));
}

#[test]
fn maven_pom_strips_comments_expands_properties_and_handles_dependency_management() {
    let tmp = TempDir::new("maven_props_and_mgmt");
    tmp.write(
        "pom.xml",
        r#"<project>
                <modelVersion>4.0.0</modelVersion>
                <parent>
                    <groupId>com.example</groupId>
                    <artifactId>parent-pom</artifactId>
                    <version>9.9.9</version>
                </parent>
                <artifactId>demo</artifactId>
                <version>2.4.0</version>
                <!--
                <properties>
                    <maven.compiler.release>8</maven.compiler.release>
                </properties>
                <dependency>
                    <groupId>com.commented</groupId>
                    <artifactId>ignored-dep</artifactId>
                    <version>1.0.0</version>
                </dependency>
                -->
                <properties>
                    <java.version>21</java.version>
                    <maven.compiler.release>${java.version}</maven.compiler.release>
                    <guava.group>com.google.guava</guava.group>
                    <guava.version>33.4.0-jre</guava.version>
                    <junit.version>5.10.2</junit.version>
                    <custom.src>custom/src</custom.src>
                    <custom.test>custom/test</custom.test>
                    <native.classifier>linux-x86_64</native.classifier>
                </properties>
                <build>
                    <sourceDirectory>${custom.src}</sourceDirectory>
                    <testSourceDirectory>${custom.test}</testSourceDirectory>
                </build>
                <dependencyManagement>
                    <dependencies>
                        <dependency>
                            <groupId>${guava.group}</groupId>
                            <artifactId>guava</artifactId>
                            <version>${guava.version}</version>
                        </dependency>
                        <dependency>
                            <groupId>org.unused</groupId>
                            <artifactId>managed-only</artifactId>
                            <version>1.2.3</version>
                        </dependency>
                    </dependencies>
                </dependencyManagement>
                <dependencies>
                    <dependency>
                        <groupId>${guava.group}</groupId>
                        <artifactId>guava</artifactId>
                    </dependency>
                    <dependency>
                        <groupId>com.example</groupId>
                        <artifactId>sibling</artifactId>
                        <version>${project.version}</version>
                        <classifier>${native.classifier}</classifier>
                    </dependency>
                    <dependency>
                        <groupId>org.junit.jupiter</groupId>
                        <artifactId>junit-jupiter</artifactId>
                        <version>${junit.version}</version>
                        <scope>test</scope>
                    </dependency>
                    <dependency>
                        <groupId>org.unmanaged</groupId>
                        <artifactId>no-version</artifactId>
                    </dependency>
                </dependencies>
            </project>"#,
    );
    tmp.write(
        "custom/src/com/example/App.java",
        "package com.example;\nclass App {}\n",
    );
    tmp.write(
        "custom/test/com/example/AppTest.java",
        "package com.example;\nclass AppTest {}\n",
    );

    let probe = probe_workspace(&tmp.0);
    assert_eq!(probe.resolved_build, BuildToolKind::Maven);
    assert_eq!(
        probe.resolved_level,
        LanguageLevel::new(Release::JAVA_21, false)
    );

    let root_mod = &probe.layout.modules[0];
    assert!(root_mod.sources.iter().any(|s| s.rel_path == "custom/src"));
    assert!(root_mod
        .sources
        .iter()
        .any(|s| s.rel_path == "custom/test" && s.scope == RootScope::Test));

    // Managed version fallback & property expansion
    assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "com.google.guava".to_string(),
        artifact: "guava".to_string(),
        version: "33.4.0-jre".to_string(),
        classifier: None,
        scope: RootScope::Main,
    }));
    // project.version & classifier expansion
    assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "com.example".to_string(),
        artifact: "sibling".to_string(),
        version: "2.4.0".to_string(),
        classifier: Some("linux-x86_64".to_string()),
        scope: RootScope::Main,
    }));
    // Test dependency expansion
    assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "org.junit.jupiter".to_string(),
        artifact: "junit-jupiter".to_string(),
        version: "5.10.2".to_string(),
        classifier: None,
        scope: RootScope::Test,
    }));
    // Unmanaged dependency omitting version still falls back to "*"
    assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "org.unmanaged".to_string(),
        artifact: "no-version".to_string(),
        version: "*".to_string(),
        classifier: None,
        scope: RootScope::Main,
    }));
    // Managed-only and commented dependencies must NOT appear
    assert!(!root_mod.dependencies.iter().any(|d| match d {
        ModuleDependency::Maven { artifact, .. } => {
            artifact == "managed-only" || artifact == "ignored-dep"
        }
        _ => false,
    }));
}

#[test]
fn scan_ignores_site_resources_and_skips_java_fixtures_inside_resource_roots() {
    let tmp = TempDir::new("scan_resources_filter");
    tmp.write(
        "src/main/java/com/example/resources/ResourceHelper.java",
        "package com.example.resources;\npublic class ResourceHelper {}\n",
    );
    tmp.write("src/main/resources/app.properties", "k=v\n");
    tmp.write(
        "src/test/resources/fixtures/Template.java",
        "package fixtures;\npublic class Template {}\n",
    );
    tmp.write("src/site/resources/images/logo.png", "png");
    tmp.write("src/site/resources/SiteSample.java", "class SiteSample {}\n");
    tmp.write("docs/resources/guide.md", "# guide\n");
    tmp.write(
        "spi/resources/META-INF/services/com.example.Spi",
        "com.example.Impl\n",
    );

    let probe = probe_workspace(&tmp.0);
    // Only real Java sources are collected; fixtures in conventional resource roots and src/site are skipped.
    assert!(probe
        .file_paths
        .contains_key("src/main/java/com/example/resources/ResourceHelper.java"));
    assert!(!probe
        .file_paths
        .contains_key("src/test/resources/fixtures/Template.java"));
    assert!(!probe
        .file_paths
        .contains_key("src/site/resources/SiteSample.java"));

    let root_mod = probe.layout.module_by_path(".").unwrap();
    let res_paths: Vec<(&str, RootScope)> = root_mod
        .resources
        .iter()
        .map(|r| (r.rel_path.as_str(), r.scope))
        .collect();
    assert_eq!(
        res_paths,
        vec![
            ("spi/resources", RootScope::Main),
            ("src/main/resources", RootScope::Main),
            ("src/test/resources", RootScope::Test),
        ]
    );
    // Package `com.example.resources` was NOT misclassified as a resource root
    assert!(root_mod
        .sources
        .iter()
        .any(|s| s.rel_path == "src/main/java" && s.scope == RootScope::Main));
}

#[test]
fn meta_inf_dir_allows_java_sources_while_conventional_resources_skip_them() {
    let tmp = TempDir::new("meta_inf_with_java_sources");
    tmp.write("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\n");
    tmp.write(
        "src/main/java/com/example/App.java",
        "package com.example;\npublic class App {}\n",
    );
    tmp.write(
        "test/META-INF/services/com.example.Spi",
        "com.example.SpiImpl\n",
    );
    tmp.write(
        "test/com/example/FooTest.java",
        "package com.example;\npublic class FooTest {}\n",
    );
    tmp.write(
        "src/test/resources/Fixture.java",
        "package fixtures;\npublic class Fixture {}\n",
    );

    let probe = probe_workspace(&tmp.0);

    let foo_test_id = probe
        .file_paths
        .get("test/com/example/FooTest.java")
        .copied()
        .expect("FooTest.java under test/ with META-INF/ must be scanned");
    assert!(
        probe
            .file_paths
            .contains_key("src/main/java/com/example/App.java"),
        "root META-INF/ must not skip workspace .java sources"
    );
    assert!(
        !probe.file_paths.contains_key("src/test/resources/Fixture.java"),
        "conventional src/test/resources/ must continue skipping .java fixtures"
    );

    let root_mod = probe.layout.module_by_path(".").unwrap();
    assert!(
        root_mod
            .resources
            .iter()
            .any(|r| r.rel_path == "test" && r.scope == RootScope::Test),
        "test/ should be inferred as a Test ResourceRoot: {:?}",
        root_mod.resources
    );
    assert!(
        root_mod
            .resources
            .iter()
            .any(|r| r.rel_path == "." && r.scope == RootScope::Main),
        "workspace root with META-INF/ should be inferred as a Main ResourceRoot: {:?}",
        root_mod.resources
    );
    assert!(
        root_mod
            .resources
            .iter()
            .any(|r| r.rel_path == "src/test/resources" && r.scope == RootScope::Test),
        "src/test/resources should be inferred as a Test ResourceRoot: {:?}",
        root_mod.resources
    );

    let test_src = root_mod
        .sources
        .iter()
        .find(|s| s.rel_path == "test" && s.scope == RootScope::Test)
        .expect("test/ should also be inferred as a Test SourceRoot");
    assert!(
        test_src.files.contains(&foo_test_id),
        "test SourceRoot should contain FooTest.java: {:?}",
        test_src.files
    );
}

#[test]
fn maven_pom_ignores_build_reporting_and_profile_plugin_dependencies() {
    let tmp = TempDir::new("maven_ignore_plugin_deps");
    tmp.write(
        "pom.xml",
        r#"<project>
                <modelVersion>4.0.0</modelVersion>
                <groupId>com.example</groupId>
                <artifactId>demo</artifactId>
                <version>1.0.0</version>
                <properties>
                    <maven.compiler.release>8</maven.compiler.release>
                    <maven.compiler.testSource>17</maven.compiler.testSource>
                    <custom.src>custom/src</custom.src>
                </properties>
                <build>
                    <sourceDirectory>${custom.src}</sourceDirectory>
                    <pluginManagement>
                        <plugins>
                            <plugin>
                                <groupId>org.eclipse.tycho</groupId>
                                <artifactId>tycho-compiler-plugin</artifactId>
                                <dependencies>
                                    <dependency>
                                        <groupId>org.eclipse.jdt</groupId>
                                        <artifactId>org.eclipse.jdt.core</artifactId>
                                        <version>3.33.0</version>
                                    </dependency>
                                </dependencies>
                            </plugin>
                        </plugins>
                    </pluginManagement>
                    <plugins>
                        <plugin>
                            <groupId>com.github.siom79.japicmp</groupId>
                            <artifactId>japicmp-maven-plugin</artifactId>
                            <dependencies>
                                <dependency>
                                    <groupId>${project.groupId}</groupId>
                                    <artifactId>${project.artifactId}</artifactId>
                                    <version>0.0.0-OLD</version>
                                </dependency>
                            </dependencies>
                        </plugin>
                    </plugins>
                </build>
                <reporting>
                    <plugins>
                        <plugin>
                            <groupId>org.apache.maven.plugins</groupId>
                            <artifactId>maven-javadoc-plugin</artifactId>
                            <dependencies>
                                <dependency>
                                    <groupId>com.example</groupId>
                                    <artifactId>report-doclet</artifactId>
                                    <version>1.0.0</version>
                                </dependency>
                            </dependencies>
                        </plugin>
                    </plugins>
                </reporting>
                <profiles>
                    <profile>
                        <id>extra</id>
                        <dependencies>
                            <dependency>
                                <groupId>com.example</groupId>
                                <artifactId>profile-only</artifactId>
                                <version>1.0.0</version>
                            </dependency>
                        </dependencies>
                    </profile>
                </profiles>
                <dependencies>
                    <dependency>
                        <groupId>com.google.guava</groupId>
                        <artifactId>guava</artifactId>
                        <version>33.2.1-jre</version>
                    </dependency>
                </dependencies>
            </project>"#,
    );
    tmp.write(
        "custom/src/com/example/App.java",
        "package com.example;\nclass App {}\n",
    );
    tmp.write(
        "src/test/java/com/example/AppTest.java",
        "package com.example;\nclass AppTest {}\n",
    );

    let probe = probe_workspace(&tmp.0);
    let root_mod = probe.layout.module_by_path(".").unwrap();
    assert!(root_mod
        .sources
        .iter()
        .any(|s| s.rel_path == "custom/src" && s.scope == RootScope::Main));
    assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "com.google.guava".to_string(),
        artifact: "guava".to_string(),
        version: "33.2.1-jre".to_string(),
        classifier: None,
        scope: RootScope::Main,
    }));
    assert_eq!(
        root_mod.dependencies.len(),
        1,
        "plugin/reporting/profile dependencies must not leak: {:?}",
        root_mod.dependencies
    );
    assert_eq!(
        root_mod.test_language_level,
        Some(LanguageLevel::new(Release::JAVA_17, false))
    );
    assert_eq!(
        probe.language_level_for_path(&tmp.0.join("custom/src/com/example/App.java")),
        LanguageLevel::new(Release::JAVA_8, false)
    );
    assert_eq!(
        probe.language_level_for_path(&tmp.0.join("src/test/java/com/example/AppTest.java")),
        LanguageLevel::new(Release::JAVA_17, false)
    );
}

#[test]
fn maven_multi_module_inherits_props_splits_deps_and_resolves_test_language_level() {
    let tmp = TempDir::new("maven_multi_module");
    tmp.write(
        "pom.xml",
        r#"<project>
                <modelVersion>4.0.0</modelVersion>
                <groupId>com.example</groupId>
                <artifactId>parent</artifactId>
                <version>2.0.0</version>
                <packaging>pom</packaging>
                <properties>
                    <maven.compiler.release>8</maven.compiler.release>
                    <maven.compiler.testRelease>17</maven.compiler.testRelease>
                    <junit.version>5.10.2</junit.version>
                </properties>
                <dependencyManagement>
                    <dependencies>
                        <dependency>
                            <groupId>com.google.errorprone</groupId>
                            <artifactId>error_prone_annotations</artifactId>
                            <version>2.28.0</version>
                        </dependency>
                        <dependency>
                            <groupId>com.example</groupId>
                            <artifactId>overridden-lib</artifactId>
                            <version>1.0.0</version>
                        </dependency>
                    </dependencies>
                </dependencyManagement>
                <dependencies>
                    <dependency>
                        <groupId>com.example</groupId>
                        <artifactId>root-only-dep</artifactId>
                        <version>1.0.0</version>
                    </dependency>
                </dependencies>
            </project>"#,
    );
    tmp.write(
        "core/pom.xml",
        r#"<project>
                <parent>
                    <groupId>com.example</groupId>
                    <artifactId>parent</artifactId>
                    <version>2.0.0</version>
                </parent>
                <artifactId>core</artifactId>
                <dependencies>
                    <dependency>
                        <groupId>com.google.errorprone</groupId>
                        <artifactId>error_prone_annotations</artifactId>
                    </dependency>
                    <dependency>
                        <groupId>org.junit.jupiter</groupId>
                        <artifactId>junit-jupiter</artifactId>
                        <version>${junit.version}</version>
                        <scope>test</scope>
                    </dependency>
                </dependencies>
            </project>"#,
    );
    tmp.write(
        "core/src/main/java/com/example/core/Core.java",
        "package com.example.core;\npublic class Core {}\n",
    );
    tmp.write(
        "core/src/test/java/com/example/core/CoreTest.java",
        "package com.example.core;\npublic class CoreTest {}\n",
    );
    tmp.write(
        "modern/pom.xml",
        r#"<project>
                <parent>
                    <groupId>com.example</groupId>
                    <artifactId>parent</artifactId>
                    <version>2.0.0</version>
                </parent>
                <artifactId>modern</artifactId>
                <properties>
                    <maven.compiler.release>17</maven.compiler.release>
                </properties>
                <dependencyManagement>
                    <dependencies>
                        <dependency>
                            <groupId>com.example</groupId>
                            <artifactId>overridden-lib</artifactId>
                            <version>2.0.0</version>
                        </dependency>
                    </dependencies>
                </dependencyManagement>
                <dependencies>
                    <dependency>
                        <groupId>com.example</groupId>
                        <artifactId>overridden-lib</artifactId>
                    </dependency>
                    <dependency>
                        <groupId>org.slf4j</groupId>
                        <artifactId>slf4j-api</artifactId>
                        <version>2.0.12</version>
                    </dependency>
                    <dependency>
                        <groupId>com.example</groupId>
                        <artifactId>excluded-dep</artifactId>
                        <version>1.0.0</version>
                    </dependency>
                </dependencies>
            </project>"#,
    );
    tmp.write(
        "modern/src/main/java/com/example/modern/Modern.java",
        "package com.example.modern;\npublic class Modern {}\n",
    );
    tmp.write(
        "modern/src/test/java/com/example/modern/ModernTest.java",
        "package com.example.modern;\npublic class ModernTest {}\n",
    );
    tmp.write(
        ".java-analyzer.toml",
        r#"
[modules."modern"]
dependencies = { exclude = ["com.example:excluded-dep"] }
"#,
    );

    let probe = probe_workspace(&tmp.0);
    assert!(probe.conflicts.is_empty(), "conflicts: {:?}", probe.conflicts);
    assert_eq!(
        probe.resolved_level,
        LanguageLevel::new(Release::JAVA_8, false)
    );
    assert_eq!(probe.layout.modules.len(), 2);

    let core_mod = probe.layout.module_by_path("core").unwrap();
    assert_eq!(
        core_mod.language_level,
        LanguageLevel::new(Release::JAVA_8, false)
    );
    assert_eq!(
        core_mod.test_language_level,
        Some(LanguageLevel::new(Release::JAVA_17, false))
    );
    assert!(core_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "com.google.errorprone".to_string(),
        artifact: "error_prone_annotations".to_string(),
        version: "2.28.0".to_string(),
        classifier: None,
        scope: RootScope::Main,
    }));
    assert!(core_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "org.junit.jupiter".to_string(),
        artifact: "junit-jupiter".to_string(),
        version: "5.10.2".to_string(),
        classifier: None,
        scope: RootScope::Test,
    }));
    assert!(
        !core_mod.dependencies.iter().any(|d| match d {
            ModuleDependency::Maven { artifact, .. } => {
                artifact == "root-only-dep" || artifact == "slf4j-api"
            }
            _ => false,
        }),
        "root-only or sibling deps must not leak into core: {:?}",
        core_mod.dependencies
    );

    let modern_mod = probe.layout.module_by_path("modern").unwrap();
    assert_eq!(
        modern_mod.language_level,
        LanguageLevel::new(Release::JAVA_17, false)
    );
    assert_eq!(
        modern_mod.test_language_level,
        Some(LanguageLevel::new(Release::JAVA_17, false))
    );
    assert!(modern_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "com.example".to_string(),
        artifact: "overridden-lib".to_string(),
        version: "2.0.0".to_string(),
        classifier: None,
        scope: RootScope::Main,
    }));
    assert!(modern_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "org.slf4j".to_string(),
        artifact: "slf4j-api".to_string(),
        version: "2.0.12".to_string(),
        classifier: None,
        scope: RootScope::Main,
    }));
    assert!(
        !modern_mod.dependencies.iter().any(|d| match d {
            ModuleDependency::Maven { artifact, .. } => {
                artifact == "root-only-dep"
                    || artifact == "error_prone_annotations"
                    || artifact == "excluded-dep"
            }
            _ => false,
        }),
        "excluded or root/sibling deps must not appear in modern: {:?}",
        modern_mod.dependencies
    );

    assert_eq!(
        probe.language_level_for_path(&tmp.0.join("core/src/main/java/com/example/core/Core.java")),
        LanguageLevel::new(Release::JAVA_8, false)
    );
    assert_eq!(
        probe.language_level_for_path(
            &tmp.0.join("core/src/test/java/com/example/core/CoreTest.java")
        ),
        LanguageLevel::new(Release::JAVA_17, false)
    );
    assert_eq!(
        probe.language_level_for_path(
            &tmp.0.join("modern/src/main/java/com/example/modern/Modern.java")
        ),
        LanguageLevel::new(Release::JAVA_17, false)
    );
    assert_eq!(
        probe.language_level_for_path(
            &tmp.0.join("modern/src/test/java/com/example/modern/ModernTest.java")
        ),
        LanguageLevel::new(Release::JAVA_17, false)
    );
}

#[test]
fn maven_submodule_expands_project_parent_properties_in_dependencies() {
    let tmp = TempDir::new("maven_parent_props");
    tmp.write(
        "pom.xml",
        r#"<project>
                <modelVersion>4.0.0</modelVersion>
                <groupId>com.example.corp</groupId>
                <artifactId>corp-parent</artifactId>
                <version>3.4.5</version>
                <packaging>pom</packaging>
                <properties>
                    <maven.compiler.release>17</maven.compiler.release>
                </properties>
            </project>"#,
    );
    let sub_pom = r#"<project>
                <parent>
                    <groupId>com.example.corp</groupId>
                    <artifactId>corp-parent</artifactId>
                    <version>3.4.5</version>
                </parent>
                <artifactId>service</artifactId>
                <version>1.0.0-SNAPSHOT</version>
                <dependencies>
                    <dependency>
                        <groupId>${project.parent.groupId}</groupId>
                        <artifactId>shared-core</artifactId>
                        <version>${project.parent.version}</version>
                    </dependency>
                </dependencies>
            </project>"#;
    tmp.write("service/pom.xml", sub_pom);
    tmp.write(
        "service/src/main/java/com/example/Service.java",
        "package com.example;\npublic class Service {}\n",
    );

    let sub_props = parse_maven_properties(sub_pom);
    assert_eq!(
        sub_props.get("project.parent.groupId").map(String::as_str),
        Some("com.example.corp")
    );
    assert_eq!(
        sub_props.get("project.parent.artifactId").map(String::as_str),
        Some("corp-parent")
    );
    assert_eq!(
        sub_props.get("project.parent.version").map(String::as_str),
        Some("3.4.5")
    );

    let probe = probe_workspace(&tmp.0);
    let service_mod = probe.layout.module_by_path("service").unwrap();
    assert!(
        service_mod.dependencies.contains(&ModuleDependency::Maven {
            group: "com.example.corp".to_string(),
            artifact: "shared-core".to_string(),
            version: "3.4.5".to_string(),
            classifier: None,
            scope: RootScope::Main,
        }),
        "expected expanded parent groupId and version in deps: {:?}",
        service_mod.dependencies
    );
}

#[test]
fn maven_test_release_tag_and_profile_test_compile_execution() {
    // 1. Direct <testRelease>17</testRelease> in maven-compiler-plugin configuration
    let tmp_tag = TempDir::new("maven_test_release_tag");
    tmp_tag.write(
        "pom.xml",
        r#"<project>
                <modelVersion>4.0.0</modelVersion>
                <groupId>com.example</groupId>
                <artifactId>demo</artifactId>
                <version>1.0.0</version>
                <properties>
                    <maven.compiler.release>8</maven.compiler.release>
                </properties>
                <build>
                    <plugins>
                        <plugin>
                            <groupId>org.apache.maven.plugins</groupId>
                            <artifactId>maven-compiler-plugin</artifactId>
                            <configuration>
                                <testRelease>17</testRelease>
                            </configuration>
                        </plugin>
                    </plugins>
                </build>
            </project>"#,
    );
    tmp_tag.write(
        "src/main/java/com/example/App.java",
        "package com.example;\nclass App {}\n",
    );
    tmp_tag.write(
        "src/test/java/com/example/AppTest.java",
        "package com.example;\nclass AppTest {}\n",
    );

    let probe_tag = probe_workspace(&tmp_tag.0);
    let root_tag = probe_tag.layout.module_by_path(".").unwrap();
    assert_eq!(
        root_tag.language_level,
        LanguageLevel::new(Release::JAVA_8, false)
    );
    assert_eq!(
        root_tag.test_language_level,
        Some(LanguageLevel::new(Release::JAVA_17, false))
    );
    assert_eq!(
        probe_tag.language_level_for_path(&tmp_tag.0.join("src/test/java/com/example/AppTest.java")),
        LanguageLevel::new(Release::JAVA_17, false)
    );

    // 2. <profile> containing <execution> with testCompile goal and <configuration><release>17</release></configuration>
    let tmp_prof = TempDir::new("maven_profile_test_compile");
    tmp_prof.write(
        "pom.xml",
        r#"<project>
                <modelVersion>4.0.0</modelVersion>
                <groupId>com.example</groupId>
                <artifactId>demo</artifactId>
                <version>1.0.0</version>
                <properties>
                    <maven.compiler.release>8</maven.compiler.release>
                </properties>
                <profiles>
                    <profile>
                        <id>jdk17-tests</id>
                        <build>
                            <plugins>
                                <plugin>
                                    <groupId>org.apache.maven.plugins</groupId>
                                    <artifactId>maven-compiler-plugin</artifactId>
                                    <executions>
                                        <execution>
                                            <id>test-compile-11</id>
                                            <goals><goal>testCompile</goal></goals>
                                            <configuration>
                                                <release>11</release>
                                            </configuration>
                                        </execution>
                                        <execution>
                                            <id>test-compile-17</id>
                                            <goals><goal>testCompile</goal></goals>
                                            <configuration>
                                                <release>17</release>
                                            </configuration>
                                        </execution>
                                    </executions>
                                </plugin>
                            </plugins>
                        </build>
                    </profile>
                </profiles>
            </project>"#,
    );
    tmp_prof.write(
        "src/main/java/com/example/App.java",
        "package com.example;\nclass App {}\n",
    );
    tmp_prof.write(
        "src/test/java/com/example/AppTest.java",
        "package com.example;\nclass AppTest {}\n",
    );

    let probe_prof = probe_workspace(&tmp_prof.0);
    let root_prof = probe_prof.layout.module_by_path(".").unwrap();
    assert_eq!(
        root_prof.language_level,
        LanguageLevel::new(Release::JAVA_8, false)
    );
    assert_eq!(
        root_prof.test_language_level,
        Some(LanguageLevel::new(Release::JAVA_17, false))
    );
    assert_eq!(
        probe_prof.language_level_for_path(&tmp_prof.0.join("src/test/java/com/example/AppTest.java")),
        LanguageLevel::new(Release::JAVA_17, false)
    );
}

#[test]
fn skips_java_fixtures_inside_noncompilable_resource_dirs() {
    let tmp_only = TempDir::new("noncompilable_resources_only");
    tmp_only.write(
        "src/test/resources-noncompilable/Broken.java",
        "package broken;\nclass Broken { @@@ invalid syntax }\n",
    );
    tmp_only.write(
        "src/it/resources-noncompilable/module-info.java",
        "module bogus.fixture.mod { requires foo.bar; }\n",
    );

    let probe_only = probe_workspace(&tmp_only.0);
    assert!(
        probe_only.file_paths.is_empty(),
        "noncompilable resource fixtures must be skipped completely: {:?}",
        probe_only.file_paths
    );
    assert!(
        probe_only
            .layout
            .modules
            .iter()
            .all(|m| m.module_name.is_none()),
        "skipped module-info.java fixture must not produce a JPMS module name: {:?}",
        probe_only.layout.modules
    );
    assert!(
        probe_only
            .layout
            .modules
            .iter()
            .all(|m| m.resources.is_empty()),
        "resources-noncompilable must not be added as a ResourceRoot: {:?}",
        probe_only.layout.modules
    );

    // Also ensure real sources are still scanned alongside skipped resources-* subtrees
    let tmp_mixed = TempDir::new("noncompilable_resources_mixed");
    tmp_mixed.write(
        "src/main/java/com/example/App.java",
        "package com.example;\npublic class App {}\n",
    );
    tmp_mixed.write(
        "src/test/resources-noncompilable/Broken.java",
        "class Broken { @@@ }\n",
    );
    tmp_mixed.write(
        "src/test/resources-noncompilable/META-INF/MANIFEST.MF",
        "Manifest-Version: 1.0\n",
    );
    tmp_mixed.write(
        "src/it/resources-noncompilable/module-info.java",
        "module bogus.fixture.mod {}\n",
    );
    tmp_mixed.write(
        "sub/src/main/resources-filtered/Filtered.java",
        "class Filtered {}\n",
    );

    let probe_mixed = probe_workspace(&tmp_mixed.0);
    assert_eq!(probe_mixed.file_paths.len(), 1);
    assert!(probe_mixed
        .file_paths
        .contains_key("src/main/java/com/example/App.java"));
    assert!(probe_mixed
        .layout
        .modules
        .iter()
        .all(|m| m.module_name.is_none()));
    assert!(probe_mixed
        .layout
        .modules
        .iter()
        .all(|m| m.resources.is_empty()));
}

#[test]
fn ant_property_file_expands_javac_release_and_ivy_dependencies() {
    let tmp = TempDir::new("ant_prop_file_ivy");
    tmp.write(
        "version.properties",
        r#"
# Line comment with hash
! Line comment with bang

java.ver=17
junit.org: org.junit.jupiter
junit.version = 5.10.2
already.set=99
"#,
    );
    tmp.write(
        "build.xml",
        r#"<project name="demo" default="compile" basedir=".">
                <property name="already.set" value="21"/>
                <property name="props.file" value="version.properties"/>
                <property file="${props.file}"/>
                <property name="java.ver" value="8"/>
                <target name="compile">
                    <javac srcdir="src" destdir="build/classes" release="${java.ver}"/>
                </target>
            </project>"#,
    );
    tmp.write(
        "ivy.xml",
        r#"<ivy-module version="2.0">
                <info organisation="com.example" module="demo"/>
                <dependencies>
                    <dependency org="${junit.org}" name="junit-jupiter" rev="${junit.version}" conf="test->default"/>
                    <dependency org="com.example" name="pinned" rev="${already.set}" conf="compile->default"/>
                </dependencies>
            </ivy-module>"#,
    );
    tmp.write(
        "src/com/example/App.java",
        "package com.example;\npublic class App {}\n",
    );

    let probe = probe_workspace(&tmp.0);
    assert!(probe.conflicts.is_empty(), "conflicts: {:?}", probe.conflicts);
    assert_eq!(probe.resolved_build, BuildToolKind::Ant);
    assert_eq!(
        probe.resolved_level,
        LanguageLevel::new(Release::JAVA_17, false)
    );

    let root_mod = &probe.layout.modules[0];
    assert!(
        root_mod.dependencies.contains(&ModuleDependency::Maven {
            group: "org.junit.jupiter".to_string(),
            artifact: "junit-jupiter".to_string(),
            version: "5.10.2".to_string(),
            classifier: None,
            scope: RootScope::Test,
        }),
        "expected expanded ivy dependency from version.properties: {:?}",
        root_mod.dependencies
    );
    assert!(
        root_mod.dependencies.contains(&ModuleDependency::Maven {
            group: "com.example".to_string(),
            artifact: "pinned".to_string(),
            version: "21".to_string(),
            classifier: None,
            scope: RootScope::Main,
        }),
        "expected existing Ant property not to be overwritten by property file: {:?}",
        root_mod.dependencies
    );
}

#[test]
fn ant_fileset_scoped_to_path_and_classpath_and_ignores_macrodef_placeholders() {
    let tmp = TempDir::new("ant_fileset_scope");
    tmp.write(
        "build.xml",
        r#"<project name="demo" default="compile" basedir=".">
                <path id="cp">
                    <fileset dir="lib" includes="*.jar"/>
                </path>
                <macrodef name="compile-macro">
                    <attribute name="filesDir"/>
                    <attribute name="cpDir"/>
                    <sequential>
                        <javac srcdir="@{filesDir}" classpath="@{cpDir}">
                            <classpath>
                                <fileset dir="@{filesDir}"/>
                            </classpath>
                        </javac>
                    </sequential>
                </macrodef>
                <target name="dist">
                    <fileset dir="manual"/>
                </target>
                <target name="compile">
                    <javac srcdir="src" destdir="build/classes" release="17">
                        <classpath refid="cp"/>
                        <classpath>
                            <fileset dir="ext-lib" includes="*.jar"/>
                        </classpath>
                    </javac>
                </target>
            </project>"#,
    );
    tmp.write("lib/dep.jar", "");
    tmp.write("ext-lib/extra.jar", "");
    tmp.write("manual/doc.jar", "");
    tmp.write(
        "src/com/example/App.java",
        "package com.example;\npublic class App {}\n",
    );

    let probe = probe_workspace(&tmp.0);
    let root_mod = &probe.layout.modules[0];

    assert!(
        root_mod.dependencies.contains(&ModuleDependency::Path {
            path: "lib/dep.jar".to_string(),
            scope: RootScope::Main,
        }),
        "expected <path><fileset dir='lib'/></path> to be extracted: {:?}",
        root_mod.dependencies
    );
    assert!(
        root_mod.dependencies.contains(&ModuleDependency::Path {
            path: "ext-lib/extra.jar".to_string(),
            scope: RootScope::Main,
        }),
        "expected <classpath><fileset dir='ext-lib'/></classpath> to be extracted: {:?}",
        root_mod.dependencies
    );
    assert!(
        !root_mod.dependencies.iter().any(|d| match d {
            ModuleDependency::Path { path, .. } => {
                path.contains("manual") || path.contains("@{")
            }
            _ => false,
        }),
        "outside-classpath <fileset> and @{{...}} placeholders must be ignored: {:?}",
        root_mod.dependencies
    );
    assert!(
        !root_mod
            .sources
            .iter()
            .any(|s| s.rel_path.contains("@{")),
        "macrodef @{{...}} srcdir must be ignored: {:?}",
        root_mod.sources
    );
}

#[test]
fn scan_respects_gitignore_and_ignore_files_while_keeping_hidden_sources() {
    let tmp = TempDir::new("scan_ignore_and_hidden");
    // No `.git` directory — `.gitignore` must still be honored (`require_git(false)`).
    tmp.write(".gitignore", "generated-ignored/\n");
    tmp.write(".ignore", "scratch/Tmp.java\n");
    tmp.write(
        "generated-ignored/Ignored.java",
        "package ignored;\npublic class Ignored {}\n",
    );
    tmp.write(
        "scratch/Tmp.java",
        "package scratch;\npublic class Tmp {}\n",
    );
    tmp.write(
        "scratch/Keep.java",
        "package scratch;\npublic class Keep {}\n",
    );
    tmp.write(
        ".bach/src/run.bach/Main.java",
        "public class Main {}\n",
    );

    let probe = probe_workspace(&tmp.0);
    assert!(
        !probe
            .file_paths
            .contains_key("generated-ignored/Ignored.java"),
        ".gitignore must exclude generated-ignored/ even without .git/: {:?}",
        probe.file_paths
    );
    assert!(
        !probe.file_paths.contains_key("scratch/Tmp.java"),
        ".ignore must exclude scratch/Tmp.java: {:?}",
        probe.file_paths
    );
    assert!(
        probe.file_paths.contains_key("scratch/Keep.java"),
        "non-ignored sibling in scratch/ must still be scanned: {:?}",
        probe.file_paths
    );
    assert!(
        probe.file_paths.contains_key(".bach/src/run.bach/Main.java"),
        "intentional hidden dot-directory sources must still be scanned: {:?}",
        probe.file_paths
    );
}
