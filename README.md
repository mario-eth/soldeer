# SolDeer ![Rust][rust-badge] [![License: MIT][license-badge]][license]

## Version 0.1.0

### WARNING

This is not a production ready product, it's still in development and should not be used in production.

[rust-badge]: https://img.shields.io/badge/Built%20with%20-Rust-e43716.svg
[license]: https://opensource.org/licenses/MIT
[license-badge]: https://img.shields.io/badge/License-MIT-blue.svg

<p align="center">
  <img src="./soldeer.png" />
</p>

### What is SolDeer?

`SolDeer` is a package manager for solidity.
It's built in rust and relies on github releases to install the dependencies in the `dependencies` folder of a project.

`SolDeer` also create a `remappings.txt` for the solc compiler to be able to compile the project using these dependencies.

### How to compile it

`cargo build --release` and use the `soldeer` binary from `target/release/`.

### How to use it

`SolDeer` is pretty simple to use, just add it to the PATH. Create a `soldeer.toml` file to add your dependencies and then run `soldeer update` in your project folder.

Example of `soldeer.toml`

```toml
[remappings]
enabled = true

[dependencies]
"openzeppelin~v4.9.2" = "https://github.com/OpenZeppelin/openzeppelin-contracts/archive/refs/tags/v4.9.2.zip"
"uniswap-v3-periphery~v1.0.0" = "https://github.com/Uniswap/v3-periphery/archive/refs/tags/v1.0.0.zip"
```

WARNING! The `[remappings]` must be first then the `[dependencies]`. The `enabled` field is used to enable or disable the remappings.

The full list of dependencies is available [here](./all_dependencies.toml).

### TODO

- A better way to write to the TOML file.
- Parallel downloads of the dependencies.
- Error handling.
- Tests. (I know, I know...)
- Refactor the code. (lots of duplicated code).
