# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0]

## Added

## Fixed

- Associations are now reused for subsequent requests when talking to strict service class
  providers. Pooled associations were validated with a C-ECHO-RQ, but an association only
  negotiates a presentation context for the abstract syntax of the actual request, so providers
  that reject a C-ECHO-RQ on such a presentation context forced a new association for every
  single request (e.g. for every instance of a STOW-RS request).
- Associations are no longer returned to the pool after a failed or partially completed message
  exchange, as their state is unknown.

## Changed

- Idle associations are now validated by checking the socket state instead of exchanging a
  C-ECHO, saving one round trip per pooled request.

## [0.3.0] - 2026-08-13

### Added

- New `/rendered` endpoints, rendering the first instance in the requested media type.
  - Supported rendered media types are:
    - `image/jpeg` (default)
    - `image/png`
  - Support for the `quality` query parameter to control the compression for lossy formats like JPEG.
  - Support for the `window` query parameter for windowing.
  - Support for the `viewport` query parameter for cropping and scaling.
- New `/metadata` endpoints for returning metadata for a given DICOM instance.
- QIDO-RS and MWL services now support `uid-list-matching` syntax for match query parameters ([GH-46](https://github.com/UMEssen/DICOM-RST/pull/46)).
- Support for sequence attribute filtering ([GH-49](https://github.com/UMEssen/DICOM-RST/pull/49)).

### Changed

- **BREAKING**: The S3-enabled container image is now published as a tag suffix on the main image (`dicom-rst:<version>-s3`) instead of a separate `dicom-rst-s3` image.
- **BREAKING**: Docker image tags no longer include the `v` prefix: `dicom-rst:0.3.0` instead of `dicom-rst:v0.3.0`.
- Updated `dicom-rs` dependency to 0.9.0
  - Baseline support for files in deflate transfer syntaxes, such as `Deflated Explicit VR Little Endian`
- Trailing slashes in URLs are now trimmed for all endpoints before processing (`/studies/` and `/studies` are equivalent).
- Return HTTP status code 200 (OK) instead of 204 (No Content) for QIDO-RS/MWL responses where there were no matches ([GH-51](https://github.com/UMEssen/DICOM-RST/pull/51), [CP-2473](https://www.dicomstandard.org/news-dir/current/docs/cpack134/cp2473.pdf)).

### Fixed

- Correctly return 413 (Payload Too Large) if the request body exceeds the configured `max-upload-size`.
- The association pool no longer leaks semaphore permits when the association is rejected ([GH-56](https://github.com/UMEssen/DICOM-RST/issues/56)).
- WADO-RS multipart responses now use a random per-response boundary instead of the fixed string `boundary`, which could collide with binary DICOM payload content and corrupt the multipart framing (RFC 2046 Section 5.1).

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
[0.4.0]: https://github.com/UMEssen/DICOM-RST/releases/tag/v0.4.0
