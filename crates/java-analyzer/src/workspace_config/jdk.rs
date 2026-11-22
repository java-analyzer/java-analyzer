use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use ide::Release;

use super::{JdkCandidate, parse_version_string};

// ---- JDK Discovery ----

pub fn discover_jdks(preferred_release: Option<Release>) -> Vec<JdkCandidate> {
    let mut candidates = Vec::new();
    let mut seen_homes = BTreeSet::new();

    // 1. JAVA_HOME
    if let Ok(java_home) = env::var("JAVA_HOME") {
        let path = PathBuf::from(java_home);
        if is_valid_jdk_home(&path) && seen_homes.insert(path.clone()) {
            let release = probe_jdk_release(&path);
            candidates.push(JdkCandidate {
                home: path,
                release,
                source: "JAVA_HOME".to_string(),
            });
        }
    }

    // 2. Standard macOS / Linux JVM install roots
    let system_roots = [
        PathBuf::from("/Library/Java/JavaVirtualMachines"),
        PathBuf::from("/usr/lib/jvm"),
    ];
    for root in &system_roots {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                let mac_home = path.join("Contents/Home");
                let candidate_home = if is_valid_jdk_home(&mac_home) {
                    mac_home
                } else {
                    path
                };
                if is_valid_jdk_home(&candidate_home) && seen_homes.insert(candidate_home.clone()) {
                    let release = probe_jdk_release(&candidate_home);
                    candidates.push(JdkCandidate {
                        home: candidate_home,
                        release,
                        source: "system".to_string(),
                    });
                }
            }
        }
    }

    // Sort: matching preferred_release first, then highest release
    candidates.sort_by(|a, b| {
        let a_match = preferred_release.is_some() && a.release == preferred_release;
        let b_match = preferred_release.is_some() && b.release == preferred_release;
        b_match
            .cmp(&a_match)
            .then_with(|| b.release.cmp(&a.release))
    });

    candidates
}

pub fn is_valid_jdk_home(path: &Path) -> bool {
    path.join("bin/javac").is_file()
        || path.join("bin/javac.exe").is_file()
        || path.join("lib/modules").is_file()
}

pub fn probe_jdk_release(jdk_home: &Path) -> Option<Release> {
    // Fast path: read `<jdk_home>/release` file (`JAVA_VERSION="25.0.1"`)
    let release_file = jdk_home.join("release");
    if let Ok(content) = fs::read_to_string(&release_file) {
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("JAVA_VERSION=")
                && let Some(rel) = parse_version_string(val)
            {
                return Some(rel);
            }
        }
    }
    // Fallback: parse directory name
    if let Some(name) = jdk_home.file_name().and_then(|n| n.to_str())
        && let Some(rel) = parse_version_string(name)
    {
        return Some(rel);
    }
    // Fallback: invoke `javac -version`
    let javac = jdk_home.join("bin/javac");
    if javac.is_file()
        && let Ok(out) = Command::new(javac).arg("-version").output()
    {
        let text = format!(
            "{} {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        if let Some(rel) = parse_version_string(&text) {
            return Some(rel);
        }
    }
    None
}
