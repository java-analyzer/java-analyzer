use std::fs;
use std::path::Path;
use zed_extension_api::{self as zed, settings::LspSettings, LanguageServerId, Result};

const GITHUB_REPO: &str = "java-analyzer/java-analyzer";
const SERVER_NAME: &str = "java-analyzer";
const BINARY_NAME: &str = "java-analyzer";
const DIR_PREFIX: &str = "java-analyzer-";

#[derive(Debug, Clone, PartialEq, Eq)]
enum BinarySource {
    Explicit(String),
    OnPath(String),
    Managed,
}

/// Maps the host `(Os, Architecture)` to the release target triple,
/// expected archive format, and executable filename inside the archive.
fn platform_target_triple(
    os: zed::Os,
    arch: zed::Architecture,
) -> Result<(&'static str, zed::DownloadedFileType, &'static str)> {
    match (os, arch) {
        (zed::Os::Mac, zed::Architecture::Aarch64) => Ok((
            "aarch64-apple-darwin",
            zed::DownloadedFileType::GzipTar,
            "java-analyzer",
        )),
        (zed::Os::Mac, zed::Architecture::X8664) => Ok((
            "x86_64-apple-darwin",
            zed::DownloadedFileType::GzipTar,
            "java-analyzer",
        )),
        (zed::Os::Linux, zed::Architecture::X8664) => Ok((
            "x86_64-unknown-linux-musl",
            zed::DownloadedFileType::GzipTar,
            "java-analyzer",
        )),
        (zed::Os::Linux, zed::Architecture::Aarch64) => Ok((
            "aarch64-unknown-linux-musl",
            zed::DownloadedFileType::GzipTar,
            "java-analyzer",
        )),
        (zed::Os::Windows, zed::Architecture::X8664) => Ok((
            "x86_64-pc-windows-msvc",
            zed::DownloadedFileType::Zip,
            "java-analyzer.exe",
        )),
        _ => Err(format!(
            "unsupported platform for {SERVER_NAME}: {os:?} {arch:?}"
        )),
    }
}

/// Formats the expected GitHub Release archive asset name for a version and target triple.
fn expected_asset_name(
    version: &str,
    triple: &str,
    file_type: zed::DownloadedFileType,
) -> String {
    let ext = match file_type {
        zed::DownloadedFileType::Zip => "zip",
        zed::DownloadedFileType::GzipTar => "tar.gz",
        zed::DownloadedFileType::Gzip => "gz",
        zed::DownloadedFileType::Uncompressed => "bin",
    };
    format!("{DIR_PREFIX}{version}-{triple}.{ext}")
}

fn version_dir(version: &str) -> String {
    format!("{DIR_PREFIX}{version}")
}

fn binary_path_for(version: &str, triple: &str, bin_name: &str) -> String {
    format!(
        "{}/{DIR_PREFIX}{version}-{triple}/{bin_name}",
        version_dir(version)
    )
}

/// Determines binary precedence:
/// 1. Explicit `lsp.java-analyzer.binary.path` setting (non-empty)
/// 2. `java-analyzer` found on `PATH`
/// 3. Auto-downloaded managed binary from GitHub Releases
fn resolve_binary(configured: Option<String>, on_path: Option<String>) -> BinarySource {
    if let Some(path) = configured.filter(|s| !s.trim().is_empty()) {
        return BinarySource::Explicit(path);
    }
    if let Some(path) = on_path.filter(|s| !s.trim().is_empty()) {
        return BinarySource::OnPath(path);
    }
    BinarySource::Managed
}

fn remove_stale_version_dirs(base_dir: &Path, keep_dir_name: &str) {
    let Ok(entries) = fs::read_dir(base_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if name_str.starts_with(DIR_PREFIX) && name_str != keep_dir_name {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn file_exists(path: &str) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

fn set_failed_status(language_server_id: &LanguageServerId, err: &str) {
    zed::set_language_server_installation_status(
        language_server_id,
        &zed::LanguageServerInstallationStatus::Failed(err.to_string()),
    );
}

struct JavaAnalyzerExtension {
    cached_binary_path: Option<String>,
}

impl JavaAnalyzerExtension {
    fn language_server_binary_path(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
        configured: Option<String>,
    ) -> Result<String> {
        match resolve_binary(configured, worktree.which(BINARY_NAME)) {
            BinarySource::Explicit(path) | BinarySource::OnPath(path) => return Ok(path),
            BinarySource::Managed => {}
        }

        if let Some(path) = &self.cached_binary_path {
            if file_exists(path) {
                return Ok(path.clone());
            }
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let release = zed::latest_github_release(
            GITHUB_REPO,
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )
        .inspect_err(|err| set_failed_status(language_server_id, err))?;

        let (os, arch) = zed::current_platform();
        let (triple, file_type, bin_name) = platform_target_triple(os, arch)
            .inspect_err(|err| set_failed_status(language_server_id, err))?;

        let asset_name = expected_asset_name(&release.version, triple, file_type);
        let extract_dir = version_dir(&release.version);
        let binary_path = binary_path_for(&release.version, triple, bin_name);

        if !file_exists(&binary_path) {
            let asset = release
                .assets
                .iter()
                .find(|asset| asset.name == asset_name)
                .ok_or_else(|| {
                    let msg = format!(
                        "no release asset found matching `{asset_name}` in {GITHUB_REPO} {}",
                        release.version
                    );
                    set_failed_status(language_server_id, &msg);
                    msg
                })?;

            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            zed::download_file(&asset.download_url, &extract_dir, file_type)
                .map_err(|err| {
                    let msg = format!("failed to download `{asset_name}`: {err}");
                    set_failed_status(language_server_id, &msg);
                    msg
                })?;

            if !file_exists(&binary_path) {
                let msg = format!(
                    "downloaded archive `{asset_name}` did not contain expected binary `{binary_path}`"
                );
                set_failed_status(language_server_id, &msg);
                return Err(msg);
            }

            if os != zed::Os::Windows {
                zed::make_file_executable(&binary_path)
                    .inspect_err(|err| set_failed_status(language_server_id, err))?;
            }

            remove_stale_version_dirs(Path::new("."), &extract_dir);
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::None,
        );
        self.cached_binary_path = Some(binary_path.clone());
        Ok(binary_path)
    }
}

impl zed::Extension for JavaAnalyzerExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary = LspSettings::for_worktree(SERVER_NAME, worktree)
            .ok()
            .and_then(|settings| settings.binary);
        let configured = binary.as_ref().and_then(|b| b.path.clone());
        let args = binary.and_then(|b| b.arguments).unwrap_or_default();
        let command = self.language_server_binary_path(language_server_id, worktree, configured)?;

        Ok(zed::Command {
            command,
            args,
            env: worktree.shell_env(),
        })
    }
}

zed::register_extension!(JavaAnalyzerExtension);

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn platform_target_triple_maps_supported_platforms() {
        assert_eq!(
            platform_target_triple(zed::Os::Mac, zed::Architecture::Aarch64),
            Ok((
                "aarch64-apple-darwin",
                zed::DownloadedFileType::GzipTar,
                "java-analyzer"
            ))
        );
        assert_eq!(
            platform_target_triple(zed::Os::Mac, zed::Architecture::X8664),
            Ok((
                "x86_64-apple-darwin",
                zed::DownloadedFileType::GzipTar,
                "java-analyzer"
            ))
        );
        assert_eq!(
            platform_target_triple(zed::Os::Linux, zed::Architecture::X8664),
            Ok((
                "x86_64-unknown-linux-musl",
                zed::DownloadedFileType::GzipTar,
                "java-analyzer"
            ))
        );
        assert_eq!(
            platform_target_triple(zed::Os::Linux, zed::Architecture::Aarch64),
            Ok((
                "aarch64-unknown-linux-musl",
                zed::DownloadedFileType::GzipTar,
                "java-analyzer"
            ))
        );
        assert_eq!(
            platform_target_triple(zed::Os::Windows, zed::Architecture::X8664),
            Ok((
                "x86_64-pc-windows-msvc",
                zed::DownloadedFileType::Zip,
                "java-analyzer.exe"
            ))
        );
    }

    #[test]
    fn platform_target_triple_rejects_unsupported_platforms() {
        assert!(platform_target_triple(zed::Os::Windows, zed::Architecture::Aarch64).is_err());
        assert!(platform_target_triple(zed::Os::Linux, zed::Architecture::X86).is_err());
    }

    #[test]
    fn expected_asset_name_matches_release_contract() {
        assert_eq!(
            expected_asset_name(
                "v0.1.0",
                "aarch64-apple-darwin",
                zed::DownloadedFileType::GzipTar
            ),
            "java-analyzer-v0.1.0-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            expected_asset_name(
                "v0.1.0",
                "x86_64-unknown-linux-musl",
                zed::DownloadedFileType::GzipTar
            ),
            "java-analyzer-v0.1.0-x86_64-unknown-linux-musl.tar.gz"
        );
        assert_eq!(
            expected_asset_name(
                "v0.1.0",
                "x86_64-pc-windows-msvc",
                zed::DownloadedFileType::Zip
            ),
            "java-analyzer-v0.1.0-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(
            binary_path_for("v0.1.0", "aarch64-apple-darwin", "java-analyzer"),
            "java-analyzer-v0.1.0/java-analyzer-v0.1.0-aarch64-apple-darwin/java-analyzer"
        );
        assert_eq!(
            binary_path_for("v0.1.0", "x86_64-pc-windows-msvc", "java-analyzer.exe"),
            "java-analyzer-v0.1.0/java-analyzer-v0.1.0-x86_64-pc-windows-msvc/java-analyzer.exe"
        );
    }

    #[test]
    fn resolve_binary_precedence() {
        assert_eq!(
            resolve_binary(Some("/custom/ja".into()), Some("/usr/bin/ja".into())),
            BinarySource::Explicit("/custom/ja".into())
        );
        assert_eq!(
            resolve_binary(Some("   ".into()), Some("/usr/bin/ja".into())),
            BinarySource::OnPath("/usr/bin/ja".into())
        );
        assert_eq!(
            resolve_binary(None, Some("/usr/bin/ja".into())),
            BinarySource::OnPath("/usr/bin/ja".into())
        );
        assert_eq!(resolve_binary(None, None), BinarySource::Managed);
    }

    #[test]
    fn remove_stale_version_dirs_prunes_only_old_server_dirs() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!("zed_ja_prune_{unique}"));
        fs::create_dir_all(temp_root.join("java-analyzer-v0.0.9")).unwrap();
        fs::create_dir_all(temp_root.join("java-analyzer-v0.1.0")).unwrap();
        fs::create_dir_all(temp_root.join("unrelated-dir")).unwrap();

        remove_stale_version_dirs(&temp_root, "java-analyzer-v0.1.0");

        assert!(!temp_root.join("java-analyzer-v0.0.9").exists());
        assert!(temp_root.join("java-analyzer-v0.1.0").exists());
        assert!(temp_root.join("unrelated-dir").exists());

        let _ = fs::remove_dir_all(&temp_root);
    }
}
