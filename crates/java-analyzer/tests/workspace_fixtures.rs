use std::{
    env, fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use hir::{LanguageLevel, ModuleDependency, ModuleTarget, Release, RootScope};
use java_analyzer::workspace_config::{probe_workspace, BuildToolKind, Conflict};

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!(
            "java_analyzer_fixture_{}_{}_{}",
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

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn fixture_maven_single_module() {
    let dir = FixtureDir::new("maven_single");
    dir.write(
        "pom.xml",
        r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
            <modelVersion>4.0.0</modelVersion>
            <groupId>com.example</groupId>
            <artifactId>demo</artifactId>
            <version>1.0.0</version>
            <properties>
                <maven.compiler.release>21</maven.compiler.release>
                <guava.version>33.2.1-jre</guava.version>
                <junit.version>5.10.3</junit.version>
                <managed.only.version>9.9.9</managed.only.version>
            </properties>
            <dependencyManagement>
                <dependencies>
                    <dependency>
                        <groupId>com.example</groupId>
                        <artifactId>managed-only</artifactId>
                        <version>${managed.only.version}</version>
                    </dependency>
                    <dependency>
                        <groupId>com.google.guava</groupId>
                        <artifactId>guava</artifactId>
                        <version>${guava.version}</version>
                    </dependency>
                </dependencies>
            </dependencyManagement>
            <dependencies>
                <dependency>
                    <groupId>com.google.guava</groupId>
                    <artifactId>guava</artifactId>
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
    dir.write(
        "src/main/java/com/example/package-info.java",
        "package com.example;\n",
    );
    dir.write(
        "src/main/java/com/example/App.java",
        "package com.example;\npublic class App {}\n",
    );
    dir.write(
        "src/test/java/com/example/AppTest.java",
        "package com.example;\npublic class AppTest {}\n",
    );
    dir.write(
        "src/main/resources/META-INF/services/com.example.Service",
        "com.example.App\n",
    );
    dir.write(
        "src/test/resources/Fixture.java",
        "package com.example;\npublic class Fixture {}\n",
    );
    dir.write("src/site/resources/style.css", "body { color: black; }\n");

    let probe = probe_workspace(&dir.0);

    assert!(probe.conflicts.is_empty(), "conflicts: {:?}", probe.conflicts);
    assert!(
        probe.layout.package_mismatches.is_empty(),
        "package mismatches: {:?}",
        probe.layout.package_mismatches
    );
    assert_eq!(probe.resolved_build, BuildToolKind::Maven);
    assert_eq!(
        probe.resolved_level,
        LanguageLevel::new(Release::JAVA_21, false)
    );

    assert_eq!(probe.layout.modules.len(), 1);
    let root_mod = &probe.layout.modules[0];
    assert_eq!(root_mod.rel_path, ".");

    assert!(root_mod
        .sources
        .iter()
        .any(|s| s.rel_path == "src/main/java" && s.scope == RootScope::Main));
    assert!(root_mod
        .sources
        .iter()
        .any(|s| s.rel_path == "src/test/java" && s.scope == RootScope::Test));
    assert!(
        !root_mod
            .sources
            .iter()
            .any(|s| s.rel_path == "src/test/resources"),
        "Fixture.java inside src/test/resources must not turn it into a source root: {:?}",
        root_mod.sources
    );

    assert!(root_mod
        .resources
        .iter()
        .any(|r| r.rel_path == "src/main/resources" && r.scope == RootScope::Main));
    assert!(root_mod
        .resources
        .iter()
        .any(|r| r.rel_path == "src/test/resources" && r.scope == RootScope::Test));
    assert!(
        !root_mod
            .resources
            .iter()
            .any(|r| r.rel_path == "src/site/resources"),
        "src/site/resources must not become a resource root: {:?}",
        root_mod.resources
    );

    assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "com.google.guava".to_string(),
        artifact: "guava".to_string(),
        version: "33.2.1-jre".to_string(),
        classifier: None,
        scope: RootScope::Main,
    }));
    assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "org.junit.jupiter".to_string(),
        artifact: "junit-jupiter".to_string(),
        version: "5.10.3".to_string(),
        classifier: None,
        scope: RootScope::Test,
    }));
    assert!(
        !root_mod.dependencies.iter().any(|d| match d {
            ModuleDependency::Maven { artifact, .. } => artifact == "managed-only",
            _ => false,
        }),
        "dependencyManagement-only entries must not be emitted: {:?}",
        root_mod.dependencies
    );
}

#[test]
fn fixture_jpms_multi_module() {
    let dir = FixtureDir::new("jpms_multi");
    dir.write(
        "core/src/main/java/module-info.java",
        "module com.example.core {\n    exports com.example.core;\n}\n",
    );
    dir.write(
        "core/src/main/java/com/example/core/CoreService.java",
        "package com.example.core;\npublic class CoreService {}\n",
    );
    dir.write(
        "app/src/main/java/module-info.java",
        "module com.example.app {\n    requires com.example.core;\n    requires java.logging;\n}\n",
    );
    dir.write(
        "app/src/main/java/com/example/app/AppMain.java",
        "package com.example.app;\npublic class AppMain {}\n",
    );

    let probe = probe_workspace(&dir.0);

    assert!(probe.conflicts.is_empty(), "conflicts: {:?}", probe.conflicts);
    assert!(
        probe.layout.package_mismatches.is_empty(),
        "package mismatches: {:?}",
        probe.layout.package_mismatches
    );
    assert_eq!(probe.layout.modules.len(), 2);

    let core_mod = probe
        .layout
        .module_by_path("core")
        .expect("core module should be detected");
    let app_mod = probe
        .layout
        .module_by_path("app")
        .expect("app module should be detected");

    assert_eq!(core_mod.module_name.as_deref(), Some("com.example.core"));
    assert_eq!(app_mod.module_name.as_deref(), Some("com.example.app"));

    assert!(core_mod
        .sources
        .iter()
        .any(|s| s.rel_path == "core/src/main/java" && s.scope == RootScope::Main));
    assert!(app_mod
        .sources
        .iter()
        .any(|s| s.rel_path == "app/src/main/java" && s.scope == RootScope::Main));

    assert!(app_mod.dependencies.contains(&ModuleDependency::Module {
        target: ModuleTarget::Workspace(core_mod.id),
        scope: RootScope::Main,
        exported: false,
    }));
    assert!(app_mod.dependencies.contains(&ModuleDependency::Module {
        target: ModuleTarget::External("java.logging".to_string()),
        scope: RootScope::Main,
        exported: false,
    }));
}

#[test]
fn fixture_gradle_kotlin_dsl() {
    let dir = FixtureDir::new("gradle_kts");
    dir.write(
        "build.gradle.kts",
        r#"plugins {
            java
        }

        java {
            toolchain {
                languageVersion.set(JavaLanguageVersion.of(21))
            }
        }

        dependencies {
            implementation("com.google.guava:guava:33.1.0-jre")
            testImplementation("org.junit.jupiter:junit-jupiter:5.10.2")
            runtimeOnly("org.postgresql:postgresql:42.7.2")
        }"#,
    );
    dir.write(
        "src/main/java/com/example/Greeter.java",
        "package com.example;\npublic class Greeter {}\n",
    );
    dir.write(
        "src/test/java/com/example/GreeterTest.java",
        "package com.example;\npublic class GreeterTest {}\n",
    );

    let probe = probe_workspace(&dir.0);

    assert!(probe.conflicts.is_empty(), "conflicts: {:?}", probe.conflicts);
    assert!(
        probe.layout.package_mismatches.is_empty(),
        "package mismatches: {:?}",
        probe.layout.package_mismatches
    );
    assert_eq!(probe.resolved_build, BuildToolKind::Gradle);
    assert_eq!(
        probe.resolved_level,
        LanguageLevel::new(Release::JAVA_21, false)
    );
    assert_eq!(probe.layout.modules.len(), 1);

    let root_mod = &probe.layout.modules[0];
    assert!(root_mod
        .sources
        .iter()
        .any(|s| s.rel_path == "src/main/java" && s.scope == RootScope::Main));
    assert!(root_mod
        .sources
        .iter()
        .any(|s| s.rel_path == "src/test/java" && s.scope == RootScope::Test));
    assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "com.google.guava".to_string(),
        artifact: "guava".to_string(),
        version: "33.1.0-jre".to_string(),
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
    assert!(!root_mod.dependencies.iter().any(|d| match d {
        ModuleDependency::Maven { artifact, .. } => artifact == "postgresql",
        _ => false,
    }));
}

#[test]
fn fixture_ant_and_ivy_pair() {
    let dir = FixtureDir::new("ant_ivy_pair");
    dir.write(
        "build.xml",
        r#"<project name="ant-demo" default="compile" basedir=".">
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
    dir.write(
        "ivy.xml",
        r#"<ivy-module version="2.0">
            <info organisation="com.example" module="ant-demo"/>
            <dependencies>
                <dependency org="com.google.guava" name="guava" rev="33.0.0-jre" conf="compile->default"/>
                <dependency org="org.junit.jupiter" name="junit-jupiter" rev="5.10.2" conf="test->default"/>
                <dependency org="org.postgresql" name="postgresql" rev="42.7.2" conf="runtime->default"/>
            </dependencies>
        </ivy-module>"#,
    );
    dir.write("lib/local-helper.jar", "");
    dir.write(
        "src/com/example/AntApp.java",
        "package com.example;\npublic class AntApp {}\n",
    );
    dir.write(
        "test/com/example/AntAppTest.java",
        "package com.example;\npublic class AntAppTest {}\n",
    );

    let probe = probe_workspace(&dir.0);

    assert!(probe.conflicts.is_empty(), "conflicts: {:?}", probe.conflicts);
    assert!(
        probe.layout.package_mismatches.is_empty(),
        "package mismatches: {:?}",
        probe.layout.package_mismatches
    );
    assert_eq!(probe.resolved_build, BuildToolKind::Ant);
    assert_eq!(
        probe.resolved_level,
        LanguageLevel::new(Release::JAVA_17, false)
    );

    let root_mod = &probe.layout.modules[0];
    assert!(root_mod
        .sources
        .iter()
        .any(|s| s.rel_path == "src" && s.scope == RootScope::Main));
    assert!(root_mod
        .sources
        .iter()
        .any(|s| s.rel_path == "test" && s.scope == RootScope::Test));
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
    assert!(!root_mod.dependencies.iter().any(|d| match d {
        ModuleDependency::Maven { artifact, .. } => artifact == "postgresql",
        _ => false,
    }));
    assert!(root_mod.dependencies.contains(&ModuleDependency::Path {
        path: "lib/local-helper.jar".to_string(),
        scope: RootScope::Main,
    }));
}

#[test]
fn fixture_zero_config_jbang_compact_source() {
    let dir = FixtureDir::new("jbang_compact");
    dir.write(
        "Hello.java",
        "#!/usr/bin/env -S java --source 25\n//DEPS com.google.guava:guava:33.3.1-jre\n//DEPS github:google/guava#v33.3.1\n\nvoid main() {\n    IO.println(\"Hello!\");\n}\n",
    );

    let probe = probe_workspace(&dir.0);

    assert!(probe.conflicts.is_empty(), "conflicts: {:?}", probe.conflicts);
    assert!(
        probe.layout.package_mismatches.is_empty(),
        "package mismatches: {:?}",
        probe.layout.package_mismatches
    );
    assert_eq!(probe.resolved_build, BuildToolKind::None);
    assert_eq!(
        probe.resolved_level,
        LanguageLevel::new(Release::JAVA_25, false)
    );
    assert_eq!(probe.layout.modules.len(), 1);

    let root_mod = &probe.layout.modules[0];
    assert_eq!(root_mod.rel_path, ".");
    assert!(root_mod
        .sources
        .iter()
        .any(|s| s.rel_path == "." && s.scope == RootScope::Main));
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
fn fixture_intentional_conflicts_and_tier1_disambiguation() {
    let dir = FixtureDir::new("conflicts_tier1");
    dir.write(
        "pom.xml",
        "<project><properties><maven.compiler.release>17</maven.compiler.release></properties></project>",
    );
    dir.write(
        "build.gradle.kts",
        "java {\n    toolchain {\n        languageVersion.set(JavaLanguageVersion.of(21))\n    }\n}\n",
    );
    dir.write(".java-version", "25\n");
    dir.write(
        "src/main/java/com/example/Good.java",
        "package com.example;\npublic class Good {}\n",
    );
    dir.write(
        "src/main/java/com/example/BadPkg.java",
        "package com.wrong;\npublic class BadPkg {}\n",
    );

    let probe = probe_workspace(&dir.0);
    assert!(probe
        .conflicts
        .iter()
        .any(|c| matches!(c, Conflict::BuildTool(_))));
    assert!(probe
        .conflicts
        .iter()
        .any(|c| matches!(c, Conflict::Release(_))));
    assert_eq!(probe.layout.package_mismatches.len(), 1);
    assert_eq!(
        probe.layout.package_mismatches[0].path,
        "src/main/java/com/example/BadPkg.java"
    );
    assert_eq!(
        probe.layout.package_mismatches[0].declared.as_deref(),
        Some("com.wrong")
    );
    assert_eq!(probe.layout.package_mismatches[0].expected, "com.example");

    dir.write(
        ".java-analyzer.toml",
        "build = \"gradle\"\nrelease = 21\nexclude = [\"src/main/java/com/example/BadPkg.java\"]\n",
    );

    let resolved_probe = probe_workspace(&dir.0);
    assert!(
        resolved_probe.conflicts.is_empty(),
        "Tier 1 .java-analyzer.toml must clear BuildTool and Release conflicts: {:?}",
        resolved_probe.conflicts
    );
    assert_eq!(resolved_probe.resolved_build, BuildToolKind::Gradle);
    assert_eq!(
        resolved_probe.resolved_level,
        LanguageLevel::new(Release::JAVA_21, false)
    );
    assert!(
        resolved_probe.layout.package_mismatches.is_empty(),
        "Excluding the mismatched file via Tier 1 must clear PackageMismatch diagnostics: {:?}",
        resolved_probe.layout.package_mismatches
    );
}

#[test]
fn fixture_maven_multi_module_gson_style() {
    let dir = FixtureDir::new("maven_gson_style");
    dir.write(
        "pom.xml",
        r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
            <modelVersion>4.0.0</modelVersion>
            <groupId>com.google.code.gson</groupId>
            <artifactId>gson-parent</artifactId>
            <version>2.11.0</version>
            <packaging>pom</packaging>
            <properties>
                <maven.compiler.release>8</maven.compiler.release>
                <maven.compiler.testRelease>17</maven.compiler.testRelease>
                <junit.version>4.13.2</junit.version>
            </properties>
            <dependencyManagement>
                <dependencies>
                    <dependency>
                        <groupId>com.google.errorprone</groupId>
                        <artifactId>error_prone_annotations</artifactId>
                        <version>2.27.0</version>
                    </dependency>
                    <dependency>
                        <groupId>junit</groupId>
                        <artifactId>junit</artifactId>
                        <version>${junit.version}</version>
                    </dependency>
                </dependencies>
            </dependencyManagement>
            <build>
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
        </project>"#,
    );
    dir.write(
        "gson/pom.xml",
        r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
            <modelVersion>4.0.0</modelVersion>
            <parent>
                <groupId>com.google.code.gson</groupId>
                <artifactId>gson-parent</artifactId>
                <version>2.11.0</version>
            </parent>
            <artifactId>gson</artifactId>
            <build>
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
            <dependencies>
                <dependency>
                    <groupId>com.google.errorprone</groupId>
                    <artifactId>error_prone_annotations</artifactId>
                    <scope>provided</scope>
                </dependency>
                <dependency>
                    <groupId>junit</groupId>
                    <artifactId>junit</artifactId>
                    <scope>test</scope>
                </dependency>
            </dependencies>
        </project>"#,
    );
    dir.write(
        "gson/src/main/java/com/google/gson/Gson.java",
        "package com.google.gson;\npublic final class Gson {}\n",
    );
    dir.write(
        "gson/src/main/java-templates/com/google/gson/internal/GsonBuildConfig.java",
        "package com.google.gson.internal;\npublic final class GsonBuildConfig {}\n",
    );
    dir.write(
        "gson/src/test/java/com/google/gson/GsonTest.java",
        "package com.google.gson;\npublic class GsonTest {}\n",
    );
    dir.write(
        "metrics/pom.xml",
        r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
            <modelVersion>4.0.0</modelVersion>
            <parent>
                <groupId>com.google.code.gson</groupId>
                <artifactId>gson-parent</artifactId>
                <version>2.11.0</version>
            </parent>
            <artifactId>metrics</artifactId>
            <dependencies>
                <dependency>
                    <groupId>${project.groupId}</groupId>
                    <artifactId>gson</artifactId>
                    <version>${project.version}</version>
                </dependency>
            </dependencies>
        </project>"#,
    );
    dir.write(
        "metrics/src/main/java/com/google/gson/metrics/BagOfPrimitives.java",
        "package com.google.gson.metrics;\npublic class BagOfPrimitives {}\n",
    );

    let probe = probe_workspace(&dir.0);

    assert!(probe.conflicts.is_empty(), "conflicts: {:?}", probe.conflicts);
    assert!(
        probe.layout.package_mismatches.is_empty(),
        "package mismatches: {:?}",
        probe.layout.package_mismatches
    );
    assert_eq!(probe.resolved_build, BuildToolKind::Maven);
    assert_eq!(
        probe.resolved_level,
        LanguageLevel::new(Release::JAVA_8, false)
    );
    assert_eq!(probe.layout.modules.len(), 2);

    let gson_mod = probe
        .layout
        .module_by_path("gson")
        .expect("gson module should be detected");
    assert_eq!(
        gson_mod.language_level,
        LanguageLevel::new(Release::JAVA_8, false)
    );
    assert_eq!(
        gson_mod.test_language_level,
        Some(LanguageLevel::new(Release::JAVA_17, false))
    );
    assert!(
        gson_mod
            .sources
            .iter()
            .any(|s| s.rel_path == "gson/src/main/java" && s.scope == RootScope::Main),
        "gson sources: {:?}",
        gson_mod.sources
    );
    assert!(
        gson_mod
            .sources
            .iter()
            .any(|s| s.rel_path == "gson/src/main/java-templates" && s.scope == RootScope::Main),
        "gson/src/main/java-templates must belong to gson module: {:?}",
        gson_mod.sources
    );
    assert!(
        gson_mod
            .sources
            .iter()
            .any(|s| s.rel_path == "gson/src/test/java" && s.scope == RootScope::Test),
        "gson test sources: {:?}",
        gson_mod.sources
    );

    assert!(gson_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "com.google.errorprone".to_string(),
        artifact: "error_prone_annotations".to_string(),
        version: "2.27.0".to_string(),
        classifier: None,
        scope: RootScope::Main,
    }));
    assert!(gson_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "junit".to_string(),
        artifact: "junit".to_string(),
        version: "4.13.2".to_string(),
        classifier: None,
        scope: RootScope::Test,
    }));
    assert!(
        !gson_mod.dependencies.iter().any(|d| match d {
            ModuleDependency::Maven {
                artifact, version, ..
            } => artifact == "org.eclipse.jdt.core" || version == "0.0.0-OLD",
            _ => false,
        }),
        "plugin dependencies must be excluded: {:?}",
        gson_mod.dependencies
    );

    let metrics_mod = probe
        .layout
        .module_by_path("metrics")
        .expect("metrics module should be detected");
    assert!(metrics_mod.dependencies.contains(&ModuleDependency::Maven {
        group: "com.google.code.gson".to_string(),
        artifact: "gson".to_string(),
        version: "2.11.0".to_string(),
        classifier: None,
        scope: RootScope::Main,
    }));
    assert!(
        !metrics_mod.dependencies.iter().any(|d| match d {
            ModuleDependency::Maven {
                artifact, version, ..
            } => artifact == "org.eclipse.jdt.core" || version == "0.0.0-OLD",
            _ => false,
        }),
        "plugin dependencies must not leak into metrics: {:?}",
        metrics_mod.dependencies
    );

    assert_eq!(
        probe.language_level_for_path(&dir.0.join("gson/src/main/java/com/google/gson/Gson.java")),
        LanguageLevel::new(Release::JAVA_8, false)
    );
    assert_eq!(
        probe.language_level_for_path(
            &dir.0
                .join("gson/src/main/java-templates/com/google/gson/internal/GsonBuildConfig.java")
        ),
        LanguageLevel::new(Release::JAVA_8, false)
    );
    assert_eq!(
        probe.language_level_for_path(
            &dir.0.join("gson/src/test/java/com/google/gson/GsonTest.java")
        ),
        LanguageLevel::new(Release::JAVA_17, false)
    );
}
