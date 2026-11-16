use zed_extension_api::{self as zed, settings::LspSettings, LanguageServerId, Result};

const SERVER_NAME: &str = "java-analyzer";
const BINARY_NAME: &str = "java-analyzer";

/// Picks the server binary: an explicit `lsp.java-analyzer.binary.path`
/// setting wins over a binary found on `PATH`.
fn resolve_binary(configured: Option<String>, on_path: Option<String>) -> Result<String> {
    configured.or(on_path).ok_or_else(|| {
        format!(
            "`{BINARY_NAME}` not found on PATH. Install it (`cargo install --path crates/java-analyzer`) \
             or set `lsp.{SERVER_NAME}.binary.path` in your Zed settings."
        )
    })
}

struct JavaAnalyzerExtension;

impl zed::Extension for JavaAnalyzerExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary = LspSettings::for_worktree(SERVER_NAME, worktree)
            .ok()
            .and_then(|settings| settings.binary);
        let configured = binary.as_ref().and_then(|b| b.path.clone());
        let args = binary.and_then(|b| b.arguments).unwrap_or_default();

        Ok(zed::Command {
            command: resolve_binary(configured, worktree.which(BINARY_NAME))?,
            args,
            env: worktree.shell_env(),
        })
    }
}

zed::register_extension!(JavaAnalyzerExtension);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_path_wins_over_path_lookup() {
        let got = resolve_binary(Some("/custom/ja".into()), Some("/usr/bin/ja".into()));
        assert_eq!(got, Ok("/custom/ja".to_string()));
    }

    #[test]
    fn falls_back_to_path_lookup() {
        let got = resolve_binary(None, Some("/usr/bin/ja".into()));
        assert_eq!(got, Ok("/usr/bin/ja".to_string()));
    }

    #[test]
    fn errors_when_binary_is_missing() {
        let err = resolve_binary(None, None).unwrap_err();
        assert!(err.contains("not found on PATH"), "{err}");
    }
}
