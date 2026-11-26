use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::Path,
};

use flate2::read::GzDecoder;
use xtask::{
    dist::{self, DistArgs, sha256_file, sha256_hex},
    release_prep::{self, ReleasePrepArgs, validate_semver},
};
use zip::ZipArchive;

fn write_fixture_workspace(root: &Path, version: &str) {
    if let Err(err) = try_write_fixture_workspace(root, version) {
        panic!("failed to write fixture workspace: {err}");
    }
}

fn try_write_fixture_workspace(root: &Path, version: &str) -> std::io::Result<()> {
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nversion = \"{version}\"\nedition = \"2024\"\n\n[workspace.dependencies]\nanyhow = \"1\"\n"
        ),
    )?;

    fs::create_dir_all(root.join("editors/vscode"))?;
    fs::write(
        root.join("editors/vscode/package.json"),
        format!(
            "{{\n  \"name\": \"java-analyzer\",\n  \"version\": \"{version}\",\n  \"dependencies\": {{\n    \"vscode-languageclient\": \"^9.0.1\"\n  }}\n}}\n"
        ),
    )?;

    fs::create_dir_all(root.join("editors/zed"))?;
    fs::write(
        root.join("editors/zed/extension.toml"),
        format!(
            "id = \"java-analyzer\"\nname = \"Java Analyzer\"\nversion = \"{version}\"\nschema_version = 1\n"
        ),
    )?;

    fs::create_dir_all(root.join("crates/java-analyzer"))?;
    fs::write(
        root.join("crates/java-analyzer/java-analyzer.usage.kdl"),
        format!("name java-analyzer\nbin java-analyzer\nversion \"{version}\"\nabout \"test\"\n"),
    )?;

    Ok(())
}

#[test]
fn semver_validation_accepts_release_and_rc_and_rejects_malformed() {
    for valid in ["0.1.0", "1.2.3", "0.2.0-rc.1", "10.0.0-rc.12"] {
        assert!(validate_semver(valid).is_ok(), "expected valid: {valid}");
    }
    for invalid in [
        "",
        "v0.1.0",
        "0.1",
        "0.1.0.0",
        "01.0.0",
        "0.01.0",
        "0.1.0-alpha.1",
        "0.1.0-rc",
        "0.1.0-rc.",
        "0.1.0-rc.01",
    ] {
        assert!(
            validate_semver(invalid).is_err(),
            "expected invalid: {invalid}"
        );
    }
}

#[test]
fn release_prep_check_detects_manifest_drift() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_fixture_workspace(root, "0.1.0");

    let check_args = ReleasePrepArgs {
        version: None,
        check: true,
    };
    let ok = release_prep::run_in(root, &check_args).unwrap();
    assert_eq!(ok.target_version, "0.1.0");

    // Drift in editors/vscode/package.json fails --check.
    fs::write(
        root.join("editors/vscode/package.json"),
        "{\n  \"name\": \"java-analyzer\",\n  \"version\": \"0.0.9\"\n}\n",
    )
    .unwrap();
    let err = release_prep::run_in(root, &check_args).unwrap_err();
    assert!(err.contains("editors/vscode/package.json"), "{err}");

    // Drift in editors/zed/extension.toml and java-analyzer.usage.kdl are also reported.
    write_fixture_workspace(root, "0.1.0");
    fs::write(
        root.join("editors/zed/extension.toml"),
        "id = \"java-analyzer\"\nname = \"Java Analyzer\"\nversion = \"0.0.8\"\nschema_version = 1\n",
    )
    .unwrap();
    fs::write(
        root.join("crates/java-analyzer/java-analyzer.usage.kdl"),
        "name java-analyzer\nbin java-analyzer\nversion \"0.0.7\"\nabout \"test\"\n",
    )
    .unwrap();
    let err = release_prep::run_in(root, &check_args).unwrap_err();
    assert!(err.contains("editors/zed/extension.toml"), "{err}");
    assert!(
        err.contains("crates/java-analyzer/java-analyzer.usage.kdl"),
        "{err}"
    );
}

#[test]
fn release_prep_syncs_all_manifests_preserving_formatting() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_fixture_workspace(root, "0.1.0");

    let update_args = ReleasePrepArgs {
        version: Some("0.2.0-rc.1".to_string()),
        check: false,
    };
    let report = release_prep::run_in(root, &update_args).unwrap();
    assert_eq!(report.target_version, "0.2.0-rc.1");
    assert_eq!(report.updated_files.len(), 4);

    let cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("version = \"0.2.0-rc.1\""), "{cargo}");
    assert!(cargo.contains("anyhow = \"1\""), "{cargo}");

    let vscode = fs::read_to_string(root.join("editors/vscode/package.json")).unwrap();
    assert!(vscode.contains("\"version\": \"0.2.0-rc.1\""), "{vscode}");
    assert!(vscode.contains("\"vscode-languageclient\": \"^9.0.1\""), "{vscode}");

    let zed = fs::read_to_string(root.join("editors/zed/extension.toml")).unwrap();
    assert!(zed.contains("version = \"0.2.0-rc.1\""), "{zed}");

    let kdl = fs::read_to_string(root.join("crates/java-analyzer/java-analyzer.usage.kdl")).unwrap();
    assert!(kdl.contains("version \"0.2.0-rc.1\""), "{kdl}");

    // Subsequent --check passes without CHANGELOG.md.
    let checked = release_prep::run_in(
        root,
        &ReleasePrepArgs {
            version: None,
            check: true,
        },
    )
    .unwrap();
    assert_eq!(checked.target_version, "0.2.0-rc.1");

    // Re-running update to the same version is a no-op (0 updated files).
    let noop = release_prep::run_in(root, &update_args).unwrap();
    assert!(noop.updated_files.is_empty(), "{:?}", noop.updated_files);
}

#[test]
fn sha256_matches_nist_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[cfg(unix)]
#[test]
fn dist_packages_tar_gz_and_zip_with_sha256sums_and_build_info() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_fixture_workspace(root, "0.1.0");
    fs::write(root.join("README.md"), "# java-analyzer\n").unwrap();
    fs::write(root.join("LICENSE-APACHE"), "Apache-2.0\n").unwrap();
    fs::write(root.join("LICENSE-MIT"), "MIT\n").unwrap();

    let dummy_bin = root.join("dummy-java-analyzer");
    fs::write(
        &dummy_bin,
        "#!/bin/sh\nif [ \"$1\" = \"completion\" ]; then\n  echo \"# completion for $2\"\n  exit 0\nfi\necho \"java-analyzer 0.1.0\"\n",
    )
    .unwrap();
    let mut perms = fs::metadata(&dummy_bin).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&dummy_bin, perms).unwrap();

    let out_dir = root.join("out");

    // 1. Package .tar.gz archive
    let tar_out = dist::run_in(
        root,
        &DistArgs {
            bin_path: Some(dummy_bin.clone()),
            target: Some("aarch64-apple-darwin".to_string()),
            tag: Some("v0.1.0".to_string()),
            completions_bin: None,
            out_dir: out_dir.clone(),
        },
    )
    .unwrap();

    assert_eq!(
        tar_out.archive_path.file_name().unwrap().to_str().unwrap(),
        "java-analyzer-v0.1.0-aarch64-apple-darwin.tar.gz"
    );
    assert_eq!(tar_out.sha256, sha256_file(&tar_out.archive_path).unwrap());

    let tar_entries = read_tar_gz_entries(&tar_out.archive_path);
    let prefix = "java-analyzer-v0.1.0-aarch64-apple-darwin";
    for expected in [
        format!("{prefix}/java-analyzer"),
        format!("{prefix}/README.md"),
        format!("{prefix}/LICENSE-APACHE"),
        format!("{prefix}/LICENSE-MIT"),
        format!("{prefix}/completions/java-analyzer.bash"),
        format!("{prefix}/completions/_java-analyzer"),
        format!("{prefix}/completions/java-analyzer.fish"),
    ] {
        assert!(
            tar_entries.contains_key(&expected),
            "missing tar entry {expected}; got {:?}",
            tar_entries.keys().collect::<Vec<_>>()
        );
    }
    assert_eq!(
        tar_entries[&format!("{prefix}/completions/_java-analyzer")],
        b"# completion for zsh\n"
    );

    // 2. Package Windows .zip archive into the same out_dir using --completions-bin
    let dummy_exe = root.join("java-analyzer.exe");
    fs::write(&dummy_exe, b"MZ-dummy-exe").unwrap();
    let zip_out = dist::run_in(
        root,
        &DistArgs {
            bin_path: Some(dummy_exe),
            target: Some("x86_64-pc-windows-msvc".to_string()),
            tag: Some("nightly".to_string()),
            completions_bin: Some(dummy_bin.clone()),
            out_dir: out_dir.clone(),
        },
    )
    .unwrap();

    assert_eq!(
        zip_out.archive_path.file_name().unwrap().to_str().unwrap(),
        "java-analyzer-nightly-x86_64-pc-windows-msvc.zip"
    );
    let zip_file = fs::File::open(&zip_out.archive_path).unwrap();
    let mut zip = ZipArchive::new(zip_file).unwrap();
    let win_prefix = "java-analyzer-nightly-x86_64-pc-windows-msvc";
    for expected in [
        format!("{win_prefix}/java-analyzer.exe"),
        format!("{win_prefix}/README.md"),
        format!("{win_prefix}/LICENSE-APACHE"),
        format!("{win_prefix}/LICENSE-MIT"),
        format!("{win_prefix}/completions/java-analyzer.bash"),
        format!("{win_prefix}/completions/_java-analyzer"),
        format!("{win_prefix}/completions/java-analyzer.fish"),
    ] {
        let mut entry = zip.by_name(&expected).unwrap();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).unwrap();
        assert!(!content.is_empty(), "{expected} should be non-empty");
    }

    let build_info: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&zip_out.build_info_path).unwrap()).unwrap();
    assert_eq!(build_info["tag"], "nightly");
    assert!(!build_info["commit"].as_str().unwrap().is_empty());
    assert!(build_info["built_at"].as_str().unwrap().ends_with('Z'));

    // 3. Re-running .tar.gz updates its SHA256SUMS entry without duplicating lines
    dist::run_in(
        root,
        &DistArgs {
            bin_path: Some(dummy_bin),
            target: Some("aarch64-apple-darwin".to_string()),
            tag: Some("v0.1.0".to_string()),
            completions_bin: None,
            out_dir,
        },
    )
    .unwrap();

    let sums = fs::read_to_string(&tar_out.sha256sums_path).unwrap();
    let lines: Vec<&str> = sums.lines().collect();
    assert_eq!(lines.len(), 2, "expected 2 lines in SHA256SUMS:\n{sums}");
    assert!(lines.contains(&format!("{}  java-analyzer-v0.1.0-aarch64-apple-darwin.tar.gz", tar_out.sha256).as_str()));
    assert!(lines.contains(&format!("{}  java-analyzer-nightly-x86_64-pc-windows-msvc.zip", zip_out.sha256).as_str()));

    let build_info: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&tar_out.build_info_path).unwrap()).unwrap();
    assert_eq!(build_info["tag"], "v0.1.0");
    assert!(!build_info["commit"].as_str().unwrap().is_empty());
    assert!(build_info["built_at"].as_str().unwrap().ends_with('Z'));
}

fn read_tar_gz_entries(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let Ok(file) = fs::File::open(path) else {
        panic!("failed to open {}", path.display());
    };
    let mut decoder = GzDecoder::new(file);
    let mut raw = Vec::new();
    if let Err(err) = decoder.read_to_end(&mut raw) {
        panic!("failed to decompress {}: {err}", path.display());
    }

    let mut entries = BTreeMap::new();
    let mut offset = 0;
    while offset + 512 <= raw.len() {
        let header = &raw[offset..offset + 512];
        if header.iter().all(|&b| b == 0) {
            break;
        }
        let name_end = header[..100].iter().position(|&b| b == 0).unwrap_or(100);
        let Ok(name) = std::str::from_utf8(&header[..name_end]) else {
            panic!("invalid UTF-8 tar name");
        };
        let prefix_end = header[345..500].iter().position(|&b| b == 0).unwrap_or(155);
        let Ok(prefix) = std::str::from_utf8(&header[345..345 + prefix_end]) else {
            panic!("invalid UTF-8 tar prefix");
        };
        let full_name = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        let Ok(size_str) = std::str::from_utf8(&header[124..135]) else {
            panic!("invalid UTF-8 tar size");
        };
        let size_raw = size_str.trim_matches('\0').trim();
        let size = usize::from_str_radix(size_raw, 8).unwrap_or(0);
        let typeflag = header[156];
        offset += 512;

        if typeflag == b'0' {
            let data = raw[offset..offset + size].to_vec();
            entries.insert(full_name, data);
        }
        let blocks = size.div_ceil(512);
        offset += blocks * 512;
    }
    entries
}
