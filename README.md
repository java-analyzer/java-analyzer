# Java Analyzer

A Java Language Server Protocol (LSP) implementation written in Rust, heavily inspired by the architecture of `rust-analyzer`.

> [!NOTE]
> **:robot: This is an unapologetically AI generated project.** 
> 
> First and foremost, lets acknowledge that, this project stands on the sholders of Titans.
> None of it could be achieved without prior work and inspiration from the [`rust-analyzer`][1] team.
> 
> That said, My primary focus is to be able to develop Java software using a modern, efficient tooling.
> Which means, I have neither funding nor inclination to drop everything I know that allows me to 
> feed my family and lifestyle and spend next who knows how many years just working on a tooling 
> project probably nobody else  needs.
> 
> At the same time, I really love the idea of a more efficient tooling for Java developers, that I have
> at least some control over. So, this project was born. Out of frustration with existing options out there,
> I decided to create my own. With a LOT of help from AI agents.

As a Java developer, the developer tooling story is not all that bad.
We are spoiled by the excellent tooling available in the Java ecosystem -- Intellij IDEA is the Golden Standard for Java development all across the board.
There is also Eclipse, and while it was the de facto standard for Java development about a decade and a half ago, setting expectations for developer tooling for a decade, it has been now overshaddowed by Intellij IDEA.
Getting back to Eclipse now, feels like a major step back.
NetBeans lives on (barely), but stepping into NetBeans land feels like stepping into an abandoned town in the bad western movie.
And all of those IDE's are so heavyweight and opinionated.

We need a faster and more lightweight option for all of those small pet projects where pulling out your IDE feels like killing a fly with a sledgehammer.
Sometimes you just need a smart enough coding editor that knows enough of your code to help you write code better, faster, and generally gets out of your way.
Here's where [VSCode], [Zed], [Helix], [Vim]/[NeoVim], and [Emacs] come in -- you open up an editor, write few lines- or files of code and hit _run_ to see what happens.

There is [JDTLS][2] -- Java Development Tooling Server, which is a language server for Java based on Eclipse JDT.
Oracle has its own official (?) [Java Platform Extension](https://github.com/oracle/javavscode#readme) for Visual Studio Code.
Now, Intellij has entered the fray and they are bringing their own [JetBrains IntelliJ Server][3].

But they all are limited in some ways.
All of them are heavyweight, spinning up an entire Java runtime just to provide language server functionality.
JDTLS and Oracle's extension are extremely opinionateds and make it difficult to set up projects that are neither Gradle nor Maven ptojects.
Intellijs's extension simply brings Intellij's own IDE core to your project settings and is only available for paying Jintellij Ultimate subscribers.

I wanted an LSP that had lightning fast startup for most small- to medium-sized projects and that was not opinionated about project structure or general setup.
in the ideal case, you just need to open the project with your favorite editor and start coding.
LSP should not get in your way by requiring you to lay out your project in a specific way.

[Rust Analyzer][1] has been a great success in the Rust community and I have always been facinated by their architectue.
This project has been born from the quriosity and a wish to offer a similar developer experience for Java developers.


[1]: https://rust-analyzer.github.io/
[2]: https://github.com/eclipse-jdtls/eclipse.jdt.ls
[3]: https://www.jetbrains.com/idea/extension/?extension=vscode#how-to-install

[VSCode]: https://code.visualstudio.com/
[Zed]: https://zed.dev/
[Helix]: https://helix-editor.com/
[Vim]: https://www.vim.org/
[NeoVim]: https://neovim.io/
[Emacs]: https://www.gnu.org/software/emacs/

## Architecture

This project is split into several pure, decoupled crates to ensure fast, incremental analysis:
- **`syntax`**: A lossless, error-resilient Java parser producing a Concrete Syntax Tree (CST) (intended to use `rowan`).
- **`hir`**: High-level Intermediate Representation and semantic analysis powered by an incremental computation engine (intended to use `salsa`).
- **`ide`**: Editor-centric features (completion, goto definition, hover).
- **`java-analyzer`**: The stateful LSP server boundary handling IO, VFS (Virtual File System), and the language server protocol.

## Installation

### 1. Install the language server

Prebuilt binaries are published on [GitHub Releases](https://github.com/java-analyzer/java-analyzer/releases) for macOS (`aarch64`/`x86_64`), Linux (`x86_64`/`aarch64` static `musl`), and Windows (`x86_64`).

- **mise** (recommended):

  ```sh
  mise use -g github:java-analyzer/java-analyzer
  ```

- **cargo-binstall**:

  ```sh
  cargo binstall java-analyzer
  ```

- **Cargo (from GitHub)**:

  ```sh
  cargo install --git https://github.com/java-analyzer/java-analyzer java-analyzer
  ```

- **Build from source**:

  ```sh
  cargo install --path crates/java-analyzer
  ```

The server speaks LSP over stdio. To install dynamic shell completions (supported for `zsh`, `bash`, `fish`, and others):

```sh
java-analyzer completion zsh --install
```

### 2. Install an editor client

Clients live in [`editors/`](editors/README.md); see that file for full details and settings.

- **Zed**: run `zed: install dev extension` and select `editors/zed`. Requires Zed's Java extension for the Java language. Automatically downloads and updates `java-analyzer` from GitHub Releases when not overridden by `lsp.java-analyzer.binary.path` or `PATH`.
- **VSCode / Cursor / VSCodium**: install the universal `.vsix` attached to [GitHub Releases](https://github.com/java-analyzer/java-analyzer/releases) (`code --install-extension java-analyzer-<version>.vsix`), or package it locally:

  ```sh
  cd editors/vscode
  npm install && npm run package
  code --install-extension java-analyzer-0.1.0.vsix
  ```

  When `javaAnalyzer.server.path` is empty (default), the extension automatically downloads, SHA-256 verifies, and updates `java-analyzer` from GitHub Releases (controlled by `javaAnalyzer.updates.channel`), falling back to `PATH` when offline.
- **Neovim**: add `editors/nvim` to your runtimepath, then call `require("java-analyzer").setup()` (or `vim.lsp.enable("java_analyzer")` on Neovim 0.11+).
- **Helix**: merge [`editors/helix/languages.toml`](editors/helix/languages.toml) into `~/.config/helix/languages.toml` (or `.helix/languages.toml`) and verify with `hx --health java`.

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
