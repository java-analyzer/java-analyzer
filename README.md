# Java Analyzer

A Java Language Server Protocol (LSP) implementation written in Rust, heavily inspired by the architecture of `rust-analyzer`.

## Architecture

This project is split into several pure, decoupled crates to ensure fast, incremental analysis:
- **`syntax`**: A lossless, error-resilient Java parser producing a Concrete Syntax Tree (CST) (intended to use `rowan`).
- **`hir`**: High-level Intermediate Representation and semantic analysis powered by an incremental computation engine (intended to use `salsa`).
- **`ide`**: Editor-centric features (completion, goto definition, hover).
- **`java-analyzer`**: The stateful LSP server boundary handling IO, VFS (Virtual File System), and the language server protocol.

## Development

We follow a **Specification-Driven Bootstrapping Process**. Check `docs/specs/` for architectural guidelines and feature requirements before implementing.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
