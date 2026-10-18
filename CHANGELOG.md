# Changelog

## [0.4.0](https://github.com/shabaraba/gh-relay/compare/v0.3.0...v0.4.0) (2026-10-18)


### Features

* add --overwrite and --merge to template use ([#20](https://github.com/shabaraba/gh-relay/issues/20)) ([d1be643](https://github.com/shabaraba/gh-relay/commit/d1be6437bcf464ebd248501c23990c43d91f3b35))

## [0.3.0](https://github.com/shabaraba/gh-relay/compare/v0.2.0...v0.3.0) (2026-10-16)


### Features

* add clean-webhook command for stale gh webhook forward registrations ([#15](https://github.com/shabaraba/gh-relay/issues/15)) ([fce2834](https://github.com/shabaraba/gh-relay/commit/fce28346744456585eb55cb69522201c90810980))
* pick the next free port automatically when start's default is taken ([#14](https://github.com/shabaraba/gh-relay/issues/14)) ([edd95fd](https://github.com/shabaraba/gh-relay/commit/edd95fde9fabc9031a6c79c4b92f30f61e43f77a))

## [0.2.0](https://github.com/shabaraba/gh-relay/compare/v0.1.1...v0.2.0) (2026-10-16)


### Features

* add PR fix workflow kind and bundled configuration templates ([#11](https://github.com/shabaraba/gh-relay/issues/11)) ([2741eb0](https://github.com/shabaraba/gh-relay/commit/2741eb063c7750607076e7294e8e43edf23bf379))


### Bug Fixes

* send SIGINT (not SIGKILL) to the webhook forwarder on shutdown ([#12](https://github.com/shabaraba/gh-relay/issues/12)) ([80475ad](https://github.com/shabaraba/gh-relay/commit/80475ad13626ec086a326853ace54ef663fafe85))

## [0.1.1](https://github.com/shabaraba/gh-relay/compare/v0.1.0...v0.1.1) (2026-10-16)


### Bug Fixes

* match gh extension's os-arch asset naming convention ([#9](https://github.com/shabaraba/gh-relay/issues/9)) ([dd842e4](https://github.com/shabaraba/gh-relay/commit/dd842e4375aa46dc576438397f9faf0a3843dfb5))

## 0.1.0 (2026-10-16)


### Features

* add detached daemon mode (start --daemon, stop, status) ([#6](https://github.com/shabaraba/gh-relay/issues/6)) ([7280e05](https://github.com/shabaraba/gh-relay/commit/7280e059560ebf0600a97bf994f7e9dda3d6052d))
* add issue-triggered research and impl labels ([7d56082](https://github.com/shabaraba/gh-relay/commit/7d5608252c119a2b1ba71c9a26d74c25da055dbc))
* add issue-triggered research and impl labels ([5aaec8a](https://github.com/shabaraba/gh-relay/commit/5aaec8af9079d8e6e1f05600ffc735704710ff75))
* add release-please and precompiled binary upload ([#7](https://github.com/shabaraba/gh-relay/issues/7)) ([affa163](https://github.com/shabaraba/gh-relay/commit/affa163ce687e1190e2796db5723aecf2e63f5ba))
* add Rust label-driven local agent review runner ([bf50c99](https://github.com/shabaraba/gh-relay/commit/bf50c993e1c332ced93e9fe485db709034310407))
* let agents pull context themselves, remove labels on delivery ([e3adb3c](https://github.com/shabaraba/gh-relay/commit/e3adb3c9c6320516682d761348d63eb17345b4fa))
* let agents pull context themselves, remove labels on delivery ([666d0f0](https://github.com/shabaraba/gh-relay/commit/666d0f0b35cbd44598c428ad3283eb0d3965fe12))
