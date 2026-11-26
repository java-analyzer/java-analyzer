# Editor integrations

All clients launch the `java-analyzer` binary and talk LSP over stdio.

The **Zed** and **VS Code** extensions automatically download and update `java-analyzer` from [GitHub Releases](https://github.com/java-analyzer/java-analyzer/releases) unless overridden. For **Neovim** and **Helix** (or if you prefer managing the binary yourself), install the server first:

```sh
# Prebuilt binary via mise (recommended)
mise use -g github:java-analyzer/java-analyzer

# Prebuilt binary via cargo-binstall
cargo binstall java-analyzer

# From GitHub via Cargo
cargo install --git https://github.com/java-analyzer/java-analyzer java-analyzer

# Or build from a local checkout
cargo install --path crates/java-analyzer
```

## Zed (`editors/zed`)

Requires the Java language to be available (install the **Java** extension in Zed).

1. Run `zed: install dev extension` from the command palette.
2. Select `editors/zed`.

Binary resolution order:
1. Explicit `lsp.java-analyzer.binary.path` setting if configured.
2. `java-analyzer` found on `PATH`.
3. Automatic download and caching of the latest release from `java-analyzer/java-analyzer` GitHub Releases (pruning older cached versions automatically).

To override the binary path:

```json
{
  "lsp": {
    "java-analyzer": {
      "binary": { "path": "/abs/path/to/java-analyzer" }
    }
  }
}
```

If the Java extension's own server also starts, pick which servers run:

```json
{
  "languages": {
    "Java": { "language_servers": ["java-analyzer", "!jdtls", "..."] }
  }
}
```

Unit tests (host target): `cd editors/zed && cargo test`.

## VSCode (`editors/vscode`)

Install the universal `.vsix` attached to [GitHub Releases](https://github.com/java-analyzer/java-analyzer/releases), or build and install it locally:

```sh
cd editors/vscode
npm install
npm test
npm run package        # produces java-analyzer-0.1.0.vsix
code --install-extension java-analyzer-0.1.0.vsix
```

When `javaAnalyzer.server.path` is empty (the default), the extension automatically downloads the matching platform archive from `java-analyzer/java-analyzer` GitHub Releases, verifies its SHA-256 checksum against `SHA256SUMS`, caches it in extension global storage, and checks for updates in the background (falling back to `java-analyzer` on `PATH` if offline or updates are disabled).

Settings:
- `javaAnalyzer.server.path`: Explicit path override (leave empty `""` for auto-managed download / `PATH`).
- `javaAnalyzer.server.args`: Extra command line arguments passed to the server.
- `javaAnalyzer.updates.channel`: `"stable"` (default), `"nightly"`, or `"off"`.
- `javaAnalyzer.trace.server`: `"off"`, `"messages"`, or `"verbose"`.

Commands:
- `Java Analyzer: Restart Server`
- `Java Analyzer: Check for Server Updates`

## Neovim (`editors/nvim`)

Add `editors/nvim` to your runtimepath (e.g. with lazy.nvim
`{ dir = "/path/to/java-analyzer/editors/nvim" }`), then:

```lua
require("java-analyzer").setup({
  -- cmd = { "/abs/path/to/java-analyzer" },
})
```

On Neovim 0.11+ you can instead skip `setup()` and just call
`vim.lsp.enable("java_analyzer")`; the config is provided by
`lsp/java_analyzer.lua`. Neovim 0.8-0.10 is supported through a `FileType`
autocmd fallback in `setup()`.

### Without the plugin directory

```lua
vim.lsp.config("java_analyzer", {
  cmd = { "java-analyzer" },
  filetypes = { "java" },
  root_markers = { ".java-analyzer.toml", "pom.xml", "build.gradle", ".jj", ".git" },
})
vim.lsp.enable("java_analyzer")
```

## Helix (`editors/helix`)

Helix includes Java tree-sitter support out of the box and defaults to `jdtls`.
To switch Java buffers to `java-analyzer`, copy or merge
[`editors/helix/languages.toml`](helix/languages.toml) into
`~/.config/helix/languages.toml` (global) or `.helix/languages.toml`
(project-local):

```toml
[language-server.java-analyzer]
command = "java-analyzer"
# Use an absolute path if `~/.cargo/bin` is not on Helix's PATH:
# command = "/abs/path/to/java-analyzer"

[[language]]
name = "java"
roots = [
  ".java-analyzer.toml",
  "pom.xml",
  "build.gradle",
  "build.gradle.kts",
  "settings.gradle",
  "settings.gradle.kts",
  "build.xml",
  "ivy.xml",
  ".jj",
  ".git",
]
language-servers = ["java-analyzer"]
```

Verify Helix picks up the binary with:

```sh
hx --health java
```
