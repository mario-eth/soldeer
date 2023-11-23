# SolDeer ![Rust][rust-badge] [![License: MIT][license-badge]][license]

## Version 0.1.5

### WARNING

#### Breaking Change 0.1.5

In this version, you can skip the creation of `soldeer.toml` if you want to use only `foundry.toml`.

Also the contents of the `soldeer.toml` were changed. Please read the documentation below.

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

`SolDeer` is straightforward. It can work with `foundry.toml` file or you can create a `soldeer.toml`. From version `0.1.5` you can skip the creation of the `soldeer.toml`
if you want to use just the `foundry.toml` file.

If you use the `soldeer.toml` approach then you need define the following parameter:

```toml
[remappings]
enabled = true

[sdependencies]

```

The `remappings` parameter instructs `SolDeer` to modify the remappings to point to the dependencies file as a configuration file.

If you use the `foundry.toml` approach then you just have to add this into your `foundry.toml` config file

```toml
[sdependencies]

```

and modify the libs parameter to include the dependencies folder

```toml
libs = ["lib", "dependencies"]
```

If `foundry.toml` is used, the remappings will be modified automatically.

### Dependency installation

Add the dependencies by using

```bash
soldeer install <dependency_name>~<version>
```

A full list of dependencies supported is available [here](https://github.com/mario-eth/soldeer-versions/blob/main/all_dependencies.toml).

Additionally, you can install a dependency from any zip file located at a specific URL:

```bash
soldeer install <dependency_name>~<version> <url>
```

This command will download the zip file of the dependency, unzip it, install it in the `dependencies` directory.

### Install dependencies from a Dependency List

#### Using soldeer.toml

To distribute a list of dependencies via `soldeer.toml`, use the `update` command:

```toml
[remappings]
  enabled = true

[sdependencies]
  @openzeppelin~v4.9.3 = "https://github.com/OpenZeppelin/openzeppelin-contracts/archive/refs/tags/v4.9.3.zip"
  "@solady~v0.0.47" = "https://github.com/Vectorized/solady/archive/refs/tags/v0.0.47.zip"
```

Assuming the above dependency list is distributed via `soldeer.toml`, the receiver should install soldeer and then run:

```bash
soldeer update
```

#### foundry.toml way

To distribute a list of dependencies via `foundry.toml`, use the `update` command

And your `foundry.toml`

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
