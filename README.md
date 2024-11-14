# Soldeer ![Rust][rust-badge] [![License: MIT][license-badge]][license]

[rust-badge]: https://img.shields.io/badge/Built%20with%20-Rust-e43716.svg
[license]: https://opensource.org/licenses/MIT
[license-badge]: https://img.shields.io/badge/License-MIT-blue.svg

<p align="center">
  <img src="https://github.com/mario-eth/soldeer/raw/main/logo/soldeer_logo_outline_512.png" />
</p>

Soldeer is a package manager for Solidity built in Rust and integrated into Foundry.

Solidity development started to become more and more complex. The need for a package manager was evident.
This project was started to solve the following issues:

- git submodules in Foundry are not a good solution for managing dependencies
- npmjs was built for the JS ecosystem, not for Solidity
- github versioning of the releases is a pain and not all the projects are using it correctly

## Installation (Foundry)

Soldeer is already integrated into Foundry. You can use it by running the following command:

```bash
forge soldeer [COMMAND]
```

To check which version of Soldeer is packaged with your Foundry install, run `forge soldeer version`.

## Installation (standalone)

Soldeer is available on [crates.io](https://crates.io/crates/soldeer) and can be installed with:

```bash
cargo install soldeer
```

### Verify installation

```bash
soldeer help
```

## Compile from Source

Clone this repository, then run `cargo build --release` inside the root.

The `soldeer` binary will be located inside the `target/release/` folder.

## Usage

Check out the [usage guide](https://github.com/mario-eth/soldeer/blob/main/USAGE.md) or
[Foundry Book](https://book.getfoundry.sh/projects/soldeer).

## Changelog

Please see the [changelog](https://github.com/mario-eth/soldeer/blob/main/CHANGES.md) for more information about each release.

## Contributing

See the [contribution guide](https://github.com/mario-eth/soldeer/blob/main/CONTRIBUTING.md) for more information.
