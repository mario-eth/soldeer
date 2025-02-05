# Usage Guide

`Soldeer` is straightforward to use. It can either be invoked from the `forge` tool provided by Foundry, or installed as
a standalone executable named `soldeer`.

Dependencies and configuration options can be specified inside Foundry's `foundry.toml` config file, or inside a
dedicated `soldeer.toml` file.

In the following sections, commands can be prefixed with `forge` to use the built-in version packaged with Foundry.

## Initializing a New Project

```bash
[forge] soldeer init [--clean]
```

The `init` command can be used to setup a project for use with Soldeer. The command will generate or modify the
project's config file (`foundry.toml` or `soldeer.toml`) and perform optional removal of Foundry-style submodule
dependencies with the `--clean` flag.

This command automatically adds the latest `forge-std` dependency to your project.

Note that Soldeer installs dependencies into a folder named `dependencies`. There is currently no way to customize this
path.

## Adding Dependencies

### From the Soldeer Registry

```bash
[forge] soldeer install <NAME>~<VERSION>
```

This command searches the Soldeer registry at [https://soldeer.xyz](https://soldeer.xyz) for the specified dependency
by name and version. If a match is found, a ZIP file containing the package source will be downloaded and unzipped into
the `dependencies` directory.

The command also adds the dependency to the project's config file and creates the necessary
[remappings](https://book.getfoundry.sh/projects/dependencies#remapping-dependencies) if configured to do so.

#### Version Requirement

The `VERSION` argument is a version requirement string and can use operators and wildcards to match a range of versions.
By default, if no operator is provided, it defaults to `=` which means "exactly this version".

Examples:

```
1.2.3         // exactly 1.2.3, equivalent to `=1.2.3`
>=1.2.3       // any version greater than or equal to 1.2.3, including any 2.x version or more
^1.2.3        // the patch and minor version can increase, but not the major
1             // any version >=1.0.0 but <2.0.0
1.2           // any version >=1.2.0 but <2.0.0
~1.2.3        // only the patch number can increase
>1.2.3,<1.4.0 // multiple requirements can be separated by a comma
```

Note that this only makes sense when used with the Soldeer registry, as it provides a list of available versions to
select from. Dependencies specified with a custom URL do not use the version requirement string in this way.

### With a Custom URL

#### ZIP file

```bash
[forge] soldeer install <NAME>~<VERSION> --url <ZIP_URL>
```

If the URL to a ZIP file is provided, the registry is not used and the file is downloaded from the URL directly. Note
that a version must still be provided, but it can be freely chosen.

#### Git Repository

```bash
[forge] soldeer install <NAME>~<VERSION> --git <GIT_URL>
```

If the URL to a git repository is provided, then the repository will be cloned into the `dependencies` folder with the 
`git` CLI available on the system. HTTPS and SSH-style URLs are supported (see examples below).

Cloning a specific identifier can be done with the `--rev <COMMIT>`, `--branch <BRANCH>` or `--tag <TAG>` arguments. If
omitted, then the default branch is checked out.

Some examples:

```bash
[forge] soldeer install test-project~v1 --git git@github.com:test/test.git
[forge] soldeer install test-project~v1 --git git@gitlab.com:test/test.git
```

```bash
[forge] soldeer install test-project~v1 --git https://github.com/test/test.git
[forge] soldeer install test-project~v1 --git https://gitlab.com/test/test.git
```

```bash
[forge] soldeer install test-project~v1 --git git@github.com:test/test.git --rev 345e611cd84bfb4e62c583fa1886c1928bc1a464
[forge] soldeer install test-project~v1 --git git@github.com:test/test.git --branch dev
[forge] soldeer install test-project~v1 --git git@github.com:test/test.git --tag v1
```

Note that a version must still be provided, but it can be freely chosen.

## Installing Existing Dependencies

```bash
[forge] soldeer install
```

When invoked without arguments, the `install` command installs the project's existing dependencies by looking at the
configuration file (`soldeer.toml`/`foundry.toml`) and lockfile `soldeer.lock` if present.

Dependencies which are already present inside the `dependencies` folder are not downloaded again. For dependencies with
a version range specified in the config file, the exact version that is written in the lockfile is used, even if a
newer version exists on the registry. To update the lockfile to use the latest supported version, use `soldeer update`.

### Recursive Installation

With the `--recursive-deps` flag, Soldeer will install the dependencies of each installed dependency, recursively.
This is done internally by running `git submodule update --init --recursive` and/or `soldeer install` inside of the
dependency's folder. This behavior can also be enabled permanently via the config file.

#### Note on Sub-Dependencies

Since each dependency is free to use its own remappings, their resolution might become tricky in case of conflicting
versions.

For example:

We have a project called `my-project` with the following dependencies:

- `dependency~1`
- `openzeppelin~5.0.2` with remapping `@openzeppelin/contracts/=dependencies/openzeppelin-5.0.2/`

A contract inside `my-project` has the following import:

```solidity
@openzeppelin/contracts/token/ERC20/ERC20.sol
```

However, `dependency~1` also depends on `openzeppelin`, but it uses version `4.9.2` (with remapping
`@openzeppelin/contracts/=dependencies/openzeppelin-4.9.2/`). The contract inside `dependency-1`
has the same import path because they chose to use the same remappings path as `my-project`:

```solidity
@openzeppelin/contracts/token/ERC20/ERC20.sol
```

This situation creates ambiguity. Furthermore, if `dependency~1` were to import a file that is no longer present in
`v5`, the compiler would give an error.

As such, we recommend to always include the version requirement string as part of the remappings path. The version
requirement string does not need to target a specific version, but could e.g. target a major version:

```toml
[profile.default]
remappings = ["@openzeppelin-contracts-5/=dependencies/@openzeppelin-contracts-5.0.2/contracts/"]

[dependencies]
"@openzeppelin-contracts" = "5"
```

```solidity
import from '@openzeppelin-contracts-5/token/ERC20/ERC20.sol';
```

This approach should ensure that the correct version (or at least a compatible version) of the included file is used.

## Updating Dependencies

```bash
[forge] soldeer update
```

For dependencies from the online registry which specify a version range, the `update` command can be used to retrieve
the latest version that matches the requirements. The `soldeer.lock` lockfile is then updated accordingly. Remappings
are automatically updated to the new version if Soldeer is configured to generate remappings.

For git dependencies which specify no identifier or a branch identifier, the `update` command checks out the latest
commit on the default or specified branch.

## Removing a Dependency

```bash
[forge] soldeer uninstall <NAME>
```

The `uninstall` command removes the dependency files and entry into the config file, lockfile and remappings.

## Publishing a Package to the Repository

```bash
[forge] soldeer push <NAME>~<VERSION>
```

In order to push a new dependency to the repository, an account must first be created at
[https://soldeer.xyz](https://soldeer.xyz). Then, a project with the dependency name must be created through the
website.

Finally, the `[forge] soldeer login` command must be used to retrieve an access token for the API.

Example:

Create a project called `my-project` and then use the `[forge] soldeer push my-project~1.0.0`. This will push the
project to the repository as version `1.0.0` and makes it available for anyone to use.

### Specifying a Path

```bash
[forge] soldeer push <NAME>~<VERSION> [PATH]
```

If the files to push are not located in the current directory, a path to the files can be provided.

### Ignoring Files

If you want to ignore certain files from the published package, you need to create one or more `.soldeerignore` files
that must contain the patterns that you want to ignore. These files can be at any level of your directory structure.
They use the `.gitignore` syntax.

Any file that matches a pattern present in `.gitignore` and `.ignore` files is also automatically excluded from the
published package.

### Dry Run

```bash
[forge] soldeer push <NAME>~<VERSION> --dry-run
```

With the `--dry-run` flag, the `push` command only creates a ZIP file containing the published package's content, but
does not upload it to the registry. The file can then be inspected to check that the contents is suitable.

We recommend that everyone runs a dry-run before pushing a new dependency to avoid publishing unwanted files.

**Warning** ⚠️

You are at risk to push sensitive files to the central repository that then can be seen by everyone. Make sure to
exclude sensitive files in the `.soldeerignore` or `.gitignore` file.

Furthermore, we've implemented a warning that gets triggered if the package contains any dotfile (a file with a
name starting with `.`). This warning can be ignored with `--skip-warnings`.

## Configuration

The `foundry.toml`/`soldeer.toml` file can have a `[soldeer]` section to configure the tool's behavior.

See the default configuration below:

```toml
[soldeer]
# whether Soldeer manages remappings
remappings_generate = true

# whether Soldeer re-generates all remappings when installing, updating or uninstalling deps
remappings_regenerate = false

# whether to suffix the remapping with the version requirement string: `name-a.b.c`
remappings_version = true

# a prefix to add to the remappings ("@" would give `@name`)
remappings_prefix = ""

# where to store the remappings ("txt" for `remappings.txt` or "config" for `foundry.toml`)
# ignored when `soldeer.toml` is used as config (uses `remappings.txt`)
remappings_location = "txt"

# whether to install sub-dependencies or not. If true this will install the dependencies of dependencies recursively.
recursive_deps = false
```

## List of Available Commands

For more commands and their usage, see `[forge] soldeer --help` and `[forge] soldeer <COMMAND> --help`.

## Remappings Caveats

If you use other dependency managers, such as git submodules or npm, ensure you don't duplicate dependencies between
soldeer and the other manager.

Remappings targeting dependencies installed without Soldeer are not modified or removed when using Soldeer commands,
unless the `--regenerate-remappings` flag is specified or the `remappings_regenerate = true` option is set.

## Dependencies Maintenance

The vision for Soldeer is that major projects such as OpenZeppelin, Solady, Uniswap would start publishing their own
packages to the Soldeer registry so that the community can easily include them and get timely updates.

Until this happens, the Soldeer maintenance team (currently m4rio.eth) will push the most popular dependencies to the
repository by relying on their npmjs or GitHub versions. We are using
[an open-source crawler tool](https://github.com/mario-eth/soldeer-crawler) to crawl and push the dependencies under the
`soldeer` organization.

For those who want an extra layer of security, the `soldeer.lock` file saves a `SHA-256` hash for each downloaded
ZIP file and the corresponding unzipped folder (see `soldeer_core::utils::hash_folder` to see how it gets generated).
These can be compared with the official releases to ensure the files were not manipulated.

**For Project Maintainers**
If you want to move your project from the Soldeer organization and take care of pushing the versions to Soldeer
yourself, please open an issue on GitHub or contact m4rio.eth on [X (formerly Twitter)](https://twitter.com/m4rio_eth).
