# Introduction

%product% is a robust DICOMweb-compatible gateway server that supports QIDO-RS, WADO-RS and STOW-RS independently of the
PACS vendor,
ensuring robust and performant transfers of large amounts of imaging data with high parallelism from multiple PACS to
multiple clients.

This documentation provides a reference for the configuration file, the provided endpoints and a user guide for
installation
and troubleshooting.

## Features

- Support for multiple PACS with parallel processing
- Support for basic DICOMweb services:
    - WADO-RS
    - QIDO-RS
    - STOW-RS
- Provides workarounds for common issues with legacy PACS
- Multiple backend implementations:
    - DIMSE
    - S3 (experimental)

The following features are planned:

- De-identification module
- Transcoding module
- Rendering module
- Proxy Backend
