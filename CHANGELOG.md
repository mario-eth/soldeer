# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## `soldeer-commands` - [0.8.1](https://github.com/mario-eth/soldeer/compare/soldeer-commands-v0.8.0...soldeer-commands-v0.8.1) - 2025-10-13

### Added
- *(commands)* add `soldeer clean` command ([#332](https://github.com/mario-eth/soldeer/pull/332))

## `soldeer-commands` - [0.8.0](https://github.com/mario-eth/soldeer/compare/soldeer-commands-v0.7.1...soldeer-commands-v0.8.0) - 2025-09-29

### Added
- add support for private packages ([#327](https://github.com/mario-eth/soldeer/pull/327))

## `soldeer-core` - [0.8.0](https://github.com/mario-eth/soldeer/compare/soldeer-core-v0.7.1...soldeer-core-v0.8.0) - 2025-09-29

### Added
- add support for private packages ([#327](https://github.com/mario-eth/soldeer/pull/327))

## `soldeer-core` - [0.7.1](https://github.com/mario-eth/soldeer/compare/soldeer-core-v0.7.0...soldeer-core-v0.7.1) - 2025-09-19

### Fixed
- *(core)* install git submodules ([#328](https://github.com/mario-eth/soldeer/pull/328))

## `soldeer` - [0.7.0](https://github.com/mario-eth/soldeer/compare/v0.6.1...v0.7.0) - 2025-09-02

### Other
- rust edition 2024 ([#319](https://github.com/mario-eth/soldeer/pull/319))

## `soldeer-commands` - [0.7.0](https://github.com/mario-eth/soldeer/compare/soldeer-commands-v0.6.1...soldeer-commands-v0.7.0) - 2025-09-02

### Added
- *(registry)* use new API endpoints ([#318](https://github.com/mario-eth/soldeer/pull/318))
- add support for CLI tokens ([#311](https://github.com/mario-eth/soldeer/pull/311))

### Fixed
- *(cmd)* avoid panicking if logger was already initialized ([#312](https://github.com/mario-eth/soldeer/pull/312))

### Other
- rust edition 2024 ([#319](https://github.com/mario-eth/soldeer/pull/319))

## `soldeer-core` - [0.7.0](https://github.com/mario-eth/soldeer/compare/soldeer-core-v0.6.1...soldeer-core-v0.7.0) - 2025-09-02

### Added
- *(registry)* use new API endpoints ([#318](https://github.com/mario-eth/soldeer/pull/318))
- add support for CLI tokens ([#311](https://github.com/mario-eth/soldeer/pull/311))

### Fixed
- *(cmd)* avoid panicking if logger was already initialized ([#312](https://github.com/mario-eth/soldeer/pull/312))

### Other
- rust edition 2024 ([#319](https://github.com/mario-eth/soldeer/pull/319))

## `soldeer-core` - [0.6.1](https://github.com/mario-eth/soldeer/compare/soldeer-core-v0.6.0...soldeer-core-v0.6.1) - 2025-07-23

### Other
- add nix flake and fix clippy ([#301](https://github.com/mario-eth/soldeer/pull/301))
- remove bzip2 support ([#298](https://github.com/mario-eth/soldeer/pull/298))

## `soldeer` - [0.6.0](https://github.com/mario-eth/soldeer/compare/v0.5.4...v0.6.0) - 2025-07-10

### Other
- update Cargo.lock dependencies

## `soldeer-commands` - [0.6.0](https://github.com/mario-eth/soldeer/compare/soldeer-commands-v0.5.4...soldeer-commands-v0.6.0) - 2025-07-10

### Added
- *(commands)* if adding a dependency which is already present, re-install all ([#289](https://github.com/mario-eth/soldeer/pull/289))

### Fixed
- *(core)* recursive subdependencies install ([#288](https://github.com/mario-eth/soldeer/pull/288))
- *(commands)* canonicalize path in push command ([#284](https://github.com/mario-eth/soldeer/pull/284))

## `soldeer-core` - [0.6.0](https://github.com/mario-eth/soldeer/compare/soldeer-core-v0.5.4...soldeer-core-v0.6.0) - 2025-07-10

### Added
- *(core)* remove forge requirement for recursive install ([#281](https://github.com/mario-eth/soldeer/pull/281))

### Fixed
- *(core)* recursive subdependencies install ([#288](https://github.com/mario-eth/soldeer/pull/288))
- *(commands)* canonicalize path in push command ([#284](https://github.com/mario-eth/soldeer/pull/284))

## `soldeer` - [0.5.4](https://github.com/mario-eth/soldeer/compare/v0.5.3...v0.5.4) - 2025-04-27

### Other
- update Cargo.lock dependencies

## `soldeer-core` - [0.5.4](https://github.com/mario-eth/soldeer/compare/soldeer-core-v0.5.3...soldeer-core-v0.5.4) - 2025-04-27

### Fixed
- *(registry)* version resolution when no SemVer ([#271](https://github.com/mario-eth/soldeer/pull/271))

## `soldeer` - [0.5.3](https://github.com/mario-eth/soldeer/compare/v0.5.2...v0.5.3) - 2025-03-18

### Changed

- fix(core): remove hardcoded git domains by @puuuuh in https://github.com/mario-eth/soldeer/pull/244
- refactor!: logging by @beeb in https://github.com/mario-eth/soldeer/pull/242
- fix(push): ensure version is non-empty when pushing to registry by @kubkon in https://github.com/mario-eth/soldeer/pull/247
- feat!: improve toml validation by @beeb in https://github.com/mario-eth/soldeer/pull/248
- chore(deps): update deps by @beeb in https://github.com/mario-eth/soldeer/pull/257

## `soldeer` - [0.5.2](https://github.com/mario-eth/soldeer/compare/v0.5.1...v0.5.2) - 2024-11-21

### Changed

- fix(core): gitignore config for integrity checksum by @beeb in #233

## `soldeer` - [0.5.1](https://github.com/mario-eth/soldeer/compare/v0.5.0...v0.5.1) - 2024-11-13

### Changed

- fix(core): keep duplicate and orphan remappings by @beeb in #226

## `soldeer` - [0.5.0](https://github.com/mario-eth/soldeer/compare/v0.4.1...v0.5.0) - 2024-11-07

### Changed

- 185 add cli args to skip interaction for all commands by @mario-eth in #218

## `soldeer` - [0.4.1](https://github.com/mario-eth/soldeer/compare/v0.4.0...v0.4.1) - 2024-10-11

### Changed

- updated readme by @mario-eth in #209
- fix(core): all commands add the `[dependencies]` table in config if m… by @mario-eth in #214
- Add core version by @mario-eth in #210


## `soldeer` - [0.4.0](https://github.com/mario-eth/soldeer/compare/v0.3.4...v0.4.0) - 2024-10-07

### Changed

- refactor!: v0.4.0 main rewrite by @beeb in #150
- docs(core): document `auth` and `config` modules by @beeb in #175
- feat: format multiline remappings array by @beeb in #174
- docs(core): add documentation by @beeb in #177
- docs(core): add documentation by @beeb in #178
- docs(core): update and utils modules by @beeb in #179
- test(commands): init integration tests by @beeb in #180
- refactor!: minor refactor and integration tests by @beeb in #186
- test(commands): add integration test (install/uninstall) by @beeb in #190
- feat(core): improve remappings matching by @beeb in #191
- fix(core): updating git dependencies by @beeb in #192
- feat(commands): update libs in foundry config during init by @beeb in #193
- refactor: remove all unwraps by @beeb in #194
- ci: speed up test by using cargo-nextest by @beeb in #196
- perf: lock-free synchronization, add rayon by @crypdoughdoteth in #198
- feat(cli): add banner by @xyizko in #199
- refactor: use new syntax for bon builders by @beeb in #200
- ci: add nextest config by @beeb in #201
- test(commands): integration tests for push by @beeb in #197
- fix(core): `path_matches` semver comparison by @beeb in #205
- fix(cli): respect environment and tty preference for color by @beeb in #206
- test(commands): fix tests when run with `cargo test` by @beeb in #207

## `soldeer` - [0.3.4](https://github.com/mario-eth/soldeer/compare/v0.3.3...v0.3.4) - 2024-09-04

### Changed

- Moving the canonicalization to respect windows slashing by @mario-eth in #172

## `soldeer` - [0.3.3](https://github.com/mario-eth/soldeer/compare/v0.3.2...v0.3.3) - 2024-09-04

### Changed

- chore(deps): bump zip-extract to 0.2.0 by @DaniPopes in #161
- fix(config): preserve existing remappings by @beeb in #171

## `soldeer` - [0.3.2](https://github.com/mario-eth/soldeer/compare/v0.3.1...v0.3.2) - 2024-08-29

### Changed

- hotfix os independent bytes by @mario-eth in #163
- remappings_generated -> remappings_generate typo by @0xCalibur in #164
- fix(utils): always consider relative path in hashing by @beeb in #168

## `soldeer` - [0.3.1](https://github.com/mario-eth/soldeer/compare/v0.3.0...v0.3.1) - 2024-08-27

### Changed

- Hotfix on OS independent bytes on hashing

## `soldeer` - [0.3.0](https://github.com/mario-eth/soldeer/compare/v0.2.19...v0.3.0) - 2024-08-27

### Changed

- Updated readme and version by @mario-eth in #104
- 89 add soldeer uninstall by @mario-eth in #105
- Feat/soldeer init by @Solthodox in #56
- style(fmt): update formatter configuration and improve consistency by @beeb in #111
- refactor!: cleanup, more idiomatic rust by @beeb in #113
- perf(lock): better handling of missing lockfile by @beeb in #114
- refactor!: big rewrite by @beeb in #118
- fix(config)!: fix remappings logic and logging by @beeb in #125
- chore: update deps and remove serde_derive by @beeb in #129
- Handling dependency name sanitization by @mario-eth in #127
- fix: parallel downloads order by @beeb in #133
- Recursive Dependencies by @mario-eth in #136
- Removing transform git to http by @mario-eth in #137
- Hotfixes and extra tests before 0.3.0 by @mario-eth in #139
- Hotfixes after refactor and extra tests by @mario-eth in #141
- feat: add integrity checksum to lockfile by @beeb in #132
- chore: update logo by @beeb in #143
- chore: enable some more lints by @DaniPopes in #160
- chore(deps): replace simple-home-dir with home by @DaniPopes in #157
- chore: remove unused dev dep env_logger by @DaniPopes in #159
- chore(deps): replace `once_cell` with `std::sync` by @DaniPopes in #158
- Using git branch/tag to pull dependencies by @mario-eth in #147
