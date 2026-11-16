# Editor integrations

All clients launch the `java-analyzer` binary and talk LSP over stdio.

Install the server first:

```sh
cargo install --path crates/java-analyzer
```

## Zed (`editors/zed`)

Requires the Java language to be available (install the **Java** extension in Zed).

1. Run `zed: install dev extension` from the command palette.
2. Select `editors/zed`.

The extension uses `java-analyzer` from `PATH`. To override it:

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

```sh
cd editors/vscode
npm install
npm run compile
npm run package        # produces java-analyzer-0.1.0.vsix
code --install-extension java-analyzer-0.1.0.vsix
```

Settings: `javaAnalyzer.server.path`, `javaAnalyzer.server.args`,
`javaAnalyzer.trace.server`. Command: `Java Analyzer: Restart Server`.

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
  root_markers = { "pom.xml", "build.gradle", ".git" },
})
vim.lsp.enable("java_analyzer")
```
