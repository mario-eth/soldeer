# Soldeer ![Rust][rust-badge] [![License: MIT][license-badge]][license]

[rust-badge]: https://img.shields.io/badge/Built%20with%20-Rust-e43716.svg
[license]: https://opensource.org/licenses/MIT
[license-badge]: https://img.shields.io/badge/License-MIT-blue.svg

<p align="center">
  <img src="./logo/soldeer_logo_outline_512.png" />
</p>

Soldeer is a package manager for Solidity built in Rust and integrated into Foundry.

Solidity development started to become more and more complex. The need for a package manager was evident.
This project was started to solve the following issues:

- git submodules in foundry are not a good solution for managing dependencies
- npmjs was built for the js ecosystem not for solidity
- github versioning of the releases is a pain and not all the projects are using it correctly

Available documentation in [USAGE](./USAGE.md) or [Foundry Book](https://book.getfoundry.sh/projects/soldeer)

## Version 0.5.0

Please see the [Changelog](./CHANGES.md) for more information.

## HOW TO INSTALL IT (FOUNDRY)

Soldeer is already integrated in foundry. You can use it by running the following command:

```bash
foundry soldeer [COMMAND]
```

## HOW TO INSTALL IT (CLI)

```bash
cargo install soldeer
```

### Check if installation was successful

```bash
soldeer help
```

## Install from sources

`cargo build --release` and use the `soldeer` binary from `target/release/`.

## HOW TO USE IT

Please see [USAGE](./USAGE.md) for more information.

## CONTRIBUTING

See [CONTRIBUTING](./CONTRIBUTING.md) for more information.
