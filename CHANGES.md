# Changelog

## Current Version 0.5.2

## Version 0.5.0 introduces the following breaking changes

[Changelog](https://github.com/mario-eth/soldeer/compare/v0.4.1..0.5.0)

## Version 0.4.0 introduces the following breaking changes

[Changelog](https://github.com/mario-eth/soldeer/commit/6dd0d97b72257ed54a52dd92182907b27e91d0bd)

## Version 0.3.0 introduces the following breaking changes

### Config file

The config file (whichever has a `[dependencies]` table between `foundry.toml` and `soldeer.toml`) now has a `[soldeer]` section with the following format and defaults:

```toml
[soldeer]
# whether soldeer manages remappings
remappings_generate = true

# whether soldeer re-generates all remappings when installing, updating or uninstalling deps
remappings_regenerate = false

# whether to suffix the remapping with the version: `name-a.b.c`
remappings_version = true

# a prefix to add to the remappings ("@" would give `@name`)
remappings_prefix = ""

# where to store the remappings ("txt" for `remappings.txt` or "config" for `foundry.toml`)
# ignored when `soldeer.toml` is used as config (uses `remappings.txt`)
remappings_location = "txt"

# whether to install sub-dependencies or not. If true this wil install the dependencies of dependencies 1 level down.
recursive_deps = false
```

### Remappings

Fully configurable Remappings, check [Remappings](#remappings-1).

#### WARNING BETA VERSION - USE AT YOUR OWN RISK

Soldeer has 3 parts:

- soldeer cli - standalone tool that can be used for managing dependencies on project, it is independent and not tied to foundry
- soldeer repository - a central repository used to store various packages. Anyone can push their own packages as public. The repository works like npmjs or crates.io
- soldeer foundry - a foundry plugin that will allow you to use soldeer in your foundry projects directly from forge: `forge soldeer [COMMAND]`

### Version 0.2.19 introduces the following breaking changes

Now you can use git to install a dependency. Supported platforms: github and gitlab.
For now, we support only public repositories.

## Version 0.2.7 introduces the following breaking changes

Save the dependency key as the dependency name to respect the Cargo.toml format. For multiple versions for the same dependency an issue has been created to be added as a feature [#34](https://github.com/mario-eth/soldeer/issues/34). For now the dependency name is the key in the toml file.

## Breaking Changes introduced in 0.2.6

In 0.2.6 the `sdependencies` has been renamed to `dependencies`. Furthermore a dependency now stored in the toml respects Cargo toml format with `version` and `url` included.
