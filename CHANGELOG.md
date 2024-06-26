# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2024-06-26

### Added

- New S3 backend with initial support for WADO-RS. STOW-RS and QIDO-RS (backed by FHIR) is planned for a future
  release.
- New documentation website hosted by GitHub Pages
- New `uncompressed` config to enforce uncompressed transfer syntaxes.
- New `graceful-shutdown` config to enable or disable graceful shutdown for the HTTP server.

### Changed
- Disabled endpoints will return 503 (Service Unavailable) instead of 404 (Not Found).

## [0.1.1] - 2024-05-22

### Fixed

- Select correct presentation context when sending PDUs

### Changed

- STORE-SCP should accept uncompressed only
- Upgrade to `dicom-rs` 0.7.0

## [0.1.0] - 2024-04-15

This is the first pre-release.
It includes basic support for QIDO-RS, WADO-RS and STOW-RS for the DIMSE backend.

### Added

- Configurable backend
- DIMSE backend
    - Implement QIDO-RS using the C-FIND protocol
    - Implement WADO-RS using the C-MOVE protocol
    - Implement STOW-RS using the C-STORE protocol
