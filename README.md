# SolDeer ![Rust][rust-badge] [![License: MIT][license-badge]][license]

## Version 0.1.4

### WARNING

This product is not production-ready. It's still in development and should not be used in production environments. Or use it at your own risk.

[rust-badge]: https://img.shields.io/badge/Built%20with%20-Rust-e43716.svg
[license]: https://opensource.org/licenses/MIT
[license-badge]: https://img.shields.io/badge/License-MIT-blue.svg

<p align="center">
  <img src="./soldeer.jpeg" />
</p>

### What is SolDeer?

`SolDeer` is a package manager for Solidity. It is built in Rust and relies on GitHub releases to install dependencies in the `dependencies` folder of a project.

`SolDeer` also creates a `remappings.txt` for the solc compiler allowing it to compile projects using these dependencies.

### How to install it

```bash
cargo install soldeer
```

The above command will install `soldeer` in your `~/.cargo/bin` folder. Ensure it's added to your PATH if it isn't already (usually it is).

#### Check if installation was succesful

```bash
soldeer help
```

### Install from sources

`cargo build --release` and use the `soldeer` binary from `target/release/`.

### How to use it

`SolDeer` is straightforward. Create a `soldeer.toml` file and define the following parameters:

```toml
[foundry]
  enabled = true
  foundry-config = false
```

These parameters instruct `SolDeer` to use the `foundry.toml` file as a configuration file. If you prefer not to use the `foundry.toml` file, set `foundry-config` to `false`, and `SolDeer` will default to the `soldeer.toml` file to manage installed dependencies.

If `enabled` is set to `false`, `remappings` won't update when a dependency is installed.

If you're using Foundry, ensure you update your `foundry.toml` to link the dependencies directory to the libs:

```toml
libs = ["lib", "dependencies"]
```

Add the dependencies by using

```bash
soldeer install <dependency_name>~<version>
```

A full list of dependencies supported is available [here](./all_dependencies.toml).

Additionally, you can install a dependency from any zip file located at a specific URL:

```bash
soldeer install <dependency_name>~<version> <url>
```

This command will download the zip file of the dependency, unzip it, install it in the `dependencies` directory, and update the dependencies tree (and add it to the remappings if `foundry->enabled == true`).

### Install dependencies from a Dependency List

#### Using soldeer.toml

To distribute a list of dependencies via `soldeer.toml`, use the `update` command:

```toml
[foundry]
  enabled = false
  foundry-config = false

[sdependencies]
  @openzeppelin~v4.9.3 = "https://github.com/OpenZeppelin/openzeppelin-contracts/archive/refs/tags/v4.9.3.zip"
  "@solady~v0.0.47" = "https://github.com/Vectorized/solady/archive/refs/tags/v0.0.47.zip"
```

Assuming the above dependency list is distributed via `soldeer.toml`, the receiver should install soldeer and then run:

```bash
soldeer update
```

#### foundry.toml way

To distribute a list of dependencies via `foundry.toml`, use the `update` command. Your `soldeer.toml` must contain:

```toml
[foundry]
  enabled = true
  foundry-config = true
```

and your `foundry.toml`

```toml
# Other foundry configs
[sdependencies]
  "@openzeppelin~v4.9.3" = "https://github.com/OpenZeppelin/openzeppelin-contracts/archive/refs/tags/v4.9.3.zip"
  "@solady~v0.0.47" = "https://github.com/Vectorized/solady/archive/refs/tags/v0.0.47.zip"
```

Assuming the above dependency list is distributed via `foundry.toml`, the receiver should install `soldeer` and then run:

```bash
soldeer update
```

#### Note: Storing the dependencies in the foundry.toml file will produce a warning when executing any forge action until forge recognizes soldeer configuration as valid within the foundry.toml file.

### CAVEATS

The "add to remappings" feature only appends to the remappings.txt file and does not delete old dependencies. If you want to remove a dependency from remappings, you must do it manually.

If you use other dependency managers, such as git submodules or npm, ensure you don't duplicate dependencies between soldeer and the other manager.

### Where are the dependencies stored?

Because many Solidity projects use npmjs to distribute their dependencies, I've created a crawler that fetches these dependencies from npmjs daily and stores them in a zip file in a GitHub repository [here](https://github.com/mario-eth/soldeer-versions/tree/main/all_versions). In the future, I plan to create a dedicated website to centralize these files, making them easier to search. Additionally, I will introduce a method to checksum the zip file against the npm or GitHub version to ensure they match. This precaution is intended to prevent the distribution of malicious code via soldeer.

The crawler code is available [here](./crawler/)