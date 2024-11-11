## How to use it

`Soldeer` is straightforward. It can work with foundry or standalone. The foundry integration uses `foundry.toml` for dependency management while the standalone uses a special toml file called `soldeer.toml`.

In the following sections we will specify all the commands with `[forge]` as prefix which means that that specific command works directly from foundry.

### DEPENDENCY INSTALLATION

Initialize a fresh installation by using this command. It will generate either a `foundry.toml` or `soldeer.toml` file with the latest version of forge-std. This command is primarily used when you want to integrate Soldeer into your Foundry project or replace your old Foundry setup with Soldeer's setup. By using the `--clean true` argument, you can delete the old `.gitmodules` file and the `lib` directory.

```bash
[forge] soldeer init
```

#### Soldeer repository

Add the dependencies by using

```bash
[forge] soldeer install <dependency_name>~<version>
```

To search if the dependency is available, visit [https://soldeer.xyz](https://soldeer.xyz).

This command will download the zip file of the dependency, unzip it, install it in the `dependencies` directory.

Additionally, you can install a dependency from any zip file located at any URL:

```bash
[forge] soldeer install <dependency_name>~<version> <url>
```

This command will download the zip file of the dependency from the <url>, unzip it, install it in the `dependencies` directory.

#### Git repositories

The syntax is `soldeer install <dependency>~<version> <git-url>`. This will clone the repository and install the dependency in the `dependencies` folder.

You can also use a certain commit as a dependency

```bash
soldeer install <dependency>~<version> --rev <commit>
```

Some example

```bash
[forge] soldeer install test-project~v1 git@github.com:test/test.git
[forge] soldeer install test-project~v1 git@gitlab.com:test/test.git
```

```bash
[forge] soldeer install test-project~v1 https://github.com/test/test.git
[forge] soldeer install test-project~v1 https://gitlab.com/test/test.git
```

Or using custom commit hashes

```bash
[forge] soldeer install test-project~v1 git@github.com:test/test.git --rev 345e611cd84bfb4e62c583fa1886c1928bc1a464
```

```bash
[forge] soldeer install
```

The command will install all the dependencies from the `soldeer.toml`/`foundry.toml` file, and will use the version inside of the `soldeer.lock` file if present.

### How to push a new dependency to the repository

In order to push a new dependency to the repository you have create an account on [https://soldeer.xyz](https://soldeer.xyz), create a project that it will match the dependency name.

Example:
Create a project called `my-project` and then use the `[forge] soldeer push my-project~1.0.0`. This will push the project to the repository and it will be available for everyone to use.
Before using the push command you have to use `[forge] soldeer login` to login to the repository.

#### Pushing a certain directory

If you want to push a certain directory from your project you can use the `[forge] soldeer push my-project~v1.0 /my/path/to/source/files` option. This will push only the files from the specified directory.

#### Ignoring files

If you want to ignore certain files from the push you need to create one or more `.soldeerignore` files that will contain the patterns that you want to ignore. These files can be at any level of your directory structure. They use the `.gitignore` syntax.

Any file that matches a pattern present in `.gitignore` and `.ignore` files is also automatically excluded.

#### Dry Run

If you want to dry run a push to inspect what files will be pushed to the central repository, use `[forge] soldeer push my-project~v1.0 [PATH_TO_DEPENDENCY] --dry-run`. This will create a zip file that you can unzip and inspect what was pushed. We recommend everyone to run a dry-run before pushing a new dependency to avoid pushing unwanted files.

**Warning** ⚠️

You are at risk to push sensitive files to the central repository that then can be seen by everyone. Make sure to exclude sensitive files in the `.soldeerignore` or `.gitignore` file.
Furthermore, we've implemented a warning that it will be triggered if you try to push a project that contains any `.dot` files/directories.
If you want to skip this warning, you can just use

```bash
[forge] soldeer push my-project~1.0.0 --skip-warnings
```

#### Remappings

The remappings are now fully configurable, the foundry/soldeer TOML files accept a
`[soldeer]` field with the following options

```toml
[soldeer]
# whether soldeer manages remappings
remappings_generate = true

# whether soldeer re-generates all remappings when installing, updating or uninstalling deps
remappings_regenerate = false

# whether to suffix the remapping with the version requirement string: `name-a.b.c`
remappings_version = true

# a prefix to add to the remappings ("@" would give `@name`)
remappings_prefix = ""

# where to store the remappings ("txt" for `remappings.txt` or "config" for `foundry.toml`)
# ignored when `soldeer.toml` is used as config (uses `remappings.txt`)
remappings_location = "txt"

# whether to install sub-dependencies or not. If true this wil install the dependencies of dependencies recursively.
recursive_deps = false
```

#### Installing dependencies of dependencies (recursive dependencies)

Whenever you install a dependency, that dependency might have other dependencies it needs to install as well. Currently, you can either specify the `recursive_deps` field as `true` inside the `[soldeer]` section or pass the `--recursive-deps` argument when calling `install` or `update`. This will trigger the installation process to go inside the dependency after installation and run `git submodule update` and `soldeer install`. By executing these commands, the dependency will pull in all the necessary dependencies for it to function properly.

##### Current issues with this

The current issue with dependencies of dependencies is that, due to improper remappings, some dependencies might not function correctly. For example:

We have a project called `my-project` with the following dependencies:

- `dependency-1`
- `openzeppelin-5.0.2`

A contract inside `my-project` has the following import:

```solidity
@openzeppelin/contracts/token/ERC20/ERC20.sol
```

However, `dependency-1` also requires `openzeppelin`, but it uses version 4.9.2. The contract inside `dependency-1` has the same import:

```solidity
@openzeppelin/contracts/token/ERC20/ERC20.sol
```

Due to improper remappings in the contract files, this situation creates ambiguity, as described above. To resolve this, we should start using versioning within imports, for example:

```solidity
import from 'openzeppelin-4.9.2/token/ERC20/ERC20.sol';
```

This approach will allow us to handle multiple versions of various dependencies effectively.

### Full list of commands

For more commands use `[forge] soldeer --help`.

### CAVEATS

The "add to remappings" feature only appends to the remappings.txt file and does not delete old dependencies. If you want to remove a dependency from remappings, you must do it manually.

If you use other dependency managers, such as git submodules or npm, ensure you don't duplicate dependencies between soldeer and the other manager. Optionally, you can set the `regenerate_remappings = true` in the config so that the remappings are generated from scratch.

### Dependencies maintenance

The goal of Soldeer is to be integrated into the pipelines of every open-source project, such as OpenZeppelin, Solady, Uniswap, etc. The maintainers of these projects can push their own dependencies to the repository, and the community can use them. Until that happens, the Soldeer maintenance team (currently m4rio.eth) will push the most used dependencies to the repository by relying on the npmjs versions or GitHub. We are using [this](https://github.com/mario-eth/soldeer-crawler) software to crawl and push the dependencies under the `soldeer` organization.

For those who want an extra layer of security, a SHA is generated in the `soldeer.lock` file for the dependencies that are installed. Some of the projects are truncated, e.g., for OpenZeppelin, only the `contracts` directory is pushed to the repository, so you will have to check the SHA against the original version's contracts directory.

**For Project Maintainers**
If you want to move your project from the Soldeer organization and take care of pushing the versions to Soldeer yourself, please open an issue or contact me on [X (formerly Twitter)](https://twitter.com/m4rio_eth).
