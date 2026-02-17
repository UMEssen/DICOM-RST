# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- QIDO-RS and MWL services now support `uid-list-matching` syntax for match query parameters. 

### Changed

- Trailing slashes in URLs are now trimmed for all endpoints before processing (`/studies/` and `/studies` are equivalent).

## [0.3.0]

### Added

- New `/rendered` endpoints, rendering the first instance in the requested media type.
    - Supported rendered media types are:
        - `image/jpeg` (default)
        - `image/png`
    - Support for the `quality` query parameter to control the compression for lossy formats like JPEG.
    - Support for the `window` query parameter for windowing.
    - Support for the `viewport` query parameter for cropping and scaling.
- New `/metadata` endpoints for returning metadata for a given DICOM instance.
- New `dicom-rst-s3` container image variant

### Changed

- Updated `dicom-rs` dependency to 0.9.0
    - Baseline support for files in deflate transfer syntaxes, such as `Deflated Explicit VR Little Endian`

## [0.2.1]

### Added

- Add `secret-key-env` and `access-key-env` options to load secrets from environment variables.
- Include [dicom-test-files](https://github.com/robyoung/dicom-test-files) as a git submodule.
- Add docker compose for a simple setup scenario with Orthanc.

### Fixed

- Return HTTP 404 "Not Found" for empty DICOM streams.

### Changed

- Rename `server.dimse.host` and `server.http.host` config key to `interface`.
- `aets.aet.host` accepts host names now, resolving to the first IP address.

## [0.2.0] - 2024-06-27

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

[0.2.0]: https://github.com/UMEssen/DICOM-RST/releases/tag/v0.2.0

[0.2.1]: https://github.com/UMEssen/DICOM-RST/releases/tag/v0.2.1

[0.3.0]: https://github.com/UMEssen/DICOM-RST/releases/tag/v0.3.0
