# Java Analyzer

A Java Language Server Protocol (LSP) implementation written in Rust, heavily inspired by the architecture of `rust-analyzer`.

## Architecture

This project is split into several pure, decoupled crates to ensure fast, incremental analysis:
- **`syntax`**: A lossless, error-resilient Java parser producing a Concrete Syntax Tree (CST) (intended to use `rowan`).
- **`hir`**: High-level Intermediate Representation and semantic analysis powered by an incremental computation engine (intended to use `salsa`).
- **`ide`**: Editor-centric features (completion, goto definition, hover).
- **`java-analyzer`**: The stateful LSP server boundary handling IO, VFS (Virtual File System), and the language server protocol.

## Installation

### 1. Install the language server

```sh
cargo install --path crates/java-analyzer
```

This puts the `java-analyzer` binary in `~/.cargo/bin`; make sure that directory is on your `PATH`. The server speaks LSP over stdio.

### 2. Install an editor client

Clients live in [`editors/`](editors/README.md); see that file for full details and settings.

- **Zed**: run `zed: install dev extension` and select `editors/zed`. Requires Zed's Java extension for the Java language.
- **VSCode**: build and install the extension.

  ```sh
  cd editors/vscode
  npm install && npm run package
  code --install-extension java-analyzer-0.1.0.vsix
  ```

- **Neovim**: add `editors/nvim` to your runtimepath, then call `require("java-analyzer").setup()` (or `vim.lsp.enable("java_analyzer")` on Neovim 0.11+).

Node.js for the VSCode client is managed with [mise](https://mise.jdx.dev): run `mise install` in the repo root.

## Development

We follow a **Specification-Driven Bootstrapping Process**. Check `docs/specs/` for architectural guidelines and feature requirements before implementing.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
