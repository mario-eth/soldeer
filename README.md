# SolDeer ![Rust][rust-badge] [![License: MIT][license-badge]][license]

## Version 0.1.2

### WARNING

This is not a production ready product, it's still in development and should not be used in production.

[rust-badge]: https://img.shields.io/badge/Built%20with%20-Rust-e43716.svg
[license]: https://opensource.org/licenses/MIT
[license-badge]: https://img.shields.io/badge/License-MIT-blue.svg

<p align="center">
  <img src="./soldeer.jpeg" />
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
[sdependencies]
"@openzeppelin~v4.9.2" = "https://github.com/OpenZeppelin/openzeppelin-contracts/archive/refs/tags/v4.9.2.zip"
"@openzeppelin~v1.0.5" = "https://github.com/OpenZeppelin/openzeppelin-contracts/archive/refs/tags/v1.0.5.zip"

[foundry]
enabled = true
foundry-config = true
```

### Foundry integration

`Soldeer` works with foundry config file as well. You just have to define the `sdependencies` option in the `foundry.toml` file.

Example

```toml
[sdependencies]
"@openzeppelin~v4.9.2" = "https://github.com/OpenZeppelin/openzeppelin-contracts/archive/refs/tags/v4.9.2.zip"
"@openzeppelin~v1.0.5" = "https://github.com/OpenZeppelin/openzeppelin-contracts/archive/refs/tags/v1.0.5.zip"
"@solady~v0.0.41" = "https://github.com/Vectorized/solady/archive/refs/tags/v0.0.41.zip"
"@uniswap-v3-periphery~v1.0.0-beta.1" = "https://github.com/Uniswap/v3-periphery/archive/refs/tags/v1.0.0-beta.1.zip"
```

#### !!! This will throw an warning when you do any `forge` action, until forge accepts `soldeer` config as a valid config within the `foundry.toml` file.

The full list of dependencies is available [here](./all_dependencies.toml).

### CAVEATS

The add to remappings feature just appends to the `remappings.txt` file, it does not delete old dependencies. So if you want to remove a dependency from remappings you have to do it manually.

### TODO

- Parallel downloads of the dependencies.
- A better way to handle the dependencies.
- Error handling.
- Tests. (I know, I know...)
- Refactor the code. (lots of duplicated code).
- skip downloading if dependency already downloaded
