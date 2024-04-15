# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-rc.1] - 2024-04-15

This is the first pre-release.
It includes basic support for QIDO-RS, WADO-RS and STOW-RS for the DIMSE backend.

### Added

- Configurable backend
- DIMSE backend
    - Implement QIDO-RS using the C-FIND protocol
    - Implement WADO-RS using the C-MOVE protocol
    - Implement STOW-RS using the C-STORE protocol
