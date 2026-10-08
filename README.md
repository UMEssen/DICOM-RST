<div align="center">

![DICOM-RST Logo[^1]](docs/images/dicom-rst-icon.png)

# DICOM-RST

**DICOMweb-compatible gateway server with DIMSE and S3 implementations.**

[Documentation](https://umessen.github.io/DICOM-RST/usage-guide.html) | [Changelog](./CHANGELOG.md)

</div>

---

DICOM-RST is a robust DICOMweb-compatible gateway server that supports QIDO-RS, WADO-RS and STOW-RS independently of the
PACS vendor, ensuring robust and performant transfers of large amounts of imaging data with high parallelism from
multiple PACS to multiple clients.

This project is part of
the [Open Medical Inference (OMI)](https://www.medizininformatik-initiative.de/de/omi-open-medical-inference) project
and is funded by the German Federal Ministry of Education and Research (BMBF)
with the funding code 01ZZ2315A-P.

<div align="center">
    <img src="docs/images/mii-omi.svg" alt="MII-Logo" width="128"/>
    <img src="docs/images/omi-logo.png" alt="OMI-Logo" width="128"/>
</div>

## Available Backends

DICOM-RST provides multiple backend implementations for the DICOMweb gateway server:

**DIMSE**:
The DIMSE backend translates DICOMweb requests into DIMSE-C messages (e.g. WADO-RS to C-MOVE).
This is the preferred backend for most users due to the broad availability and support of DIMSE services in picture
archiving and communication systems.

**S3**:
The experimental S3 backend downloads DICOM instances from S3-compatible storage. Currently, only WADO-RS
instance retrieval is implemented (no metadata or rendered resources).

## DICOMweb Features

> [!NOTE]  
> Actual support may vary depending on the features implemented by the origin server.

### Retrieve DICOM objects (WADO-RS)

https://www.dicomstandard.org/using/dicomweb/retrieve-wado-rs-and-wado-uri

#### Instance Resources

| Description      | Path                                                   | Support Status |
| ---------------- | ------------------------------------------------------ | :------------: |
| Study Instances  | `studies/{study}`                                      |       ✅       |
| Series Instances | `studies/{study}/series/{series}`                      |       ✅       |
| Instance         | `studies/{study}/series/{series}/instances/{instance}` |       ✅       |

#### Metadata Resources

| Description       | Path                                                            | Support Status |
| ----------------- | --------------------------------------------------------------- | :------------: |
| Study Metadata    | `studies/{study}/metadata`                                      |       ✅       |
| Series Metadata   | `studies/{study}/series/{series}/metadata`                      |       ✅       |
| Instance Metadata | `studies/{study}/series/{series}/instances/{instance}/metadata` |       ✅       |

Metadata is returned as DICOM JSON with bulkdata (e.g. pixel data) removed.
Metadata resources are not supported by the S3 backend.

#### Rendered Resources

| Description      | Path                                                                            | Support Status |
| ---------------- | ------------------------------------------------------------------------------- | :------------: |
| Study Instances  | `studies/{study}/rendered`                                                      |       ✅       |
| Series Instances | `studies/{study}/series/{series}/rendered`                                      |       ✅       |
| Instance         | `studies/{study}/series/{series}/instances/{instance}/rendered`                 |       ✅       |
| Frames           | `studies/{study}/series/{series}/instances/{instance}/frames/{frames}/rendered` |       ✅       |

For rendering, the first frame of the first instance with pixeldata is used.

The following query parameters are supported:

| Key      | Description                                     | Support Status |
| -------- | ----------------------------------------------- | :------------: |
| quality  | Compression quality for lossy formats like JPEG |       ✅       |
| viewport | Cropping and scaling                            |       ✅       |
| window   | Windowing (VOI LUT)                             |       ✅       |

| Media Type      | Support Status    |
| --------------- | ----------------- |
| image/jpeg      | ✅                |
| image/png       | ✅                |
| image/jp2       | ❌                |
| image/gif       | ❌ (single frame) |
| image/gif       | ❌ (multi frame)  |
| video/mpeg      | ❌                |
| video/mp4       | ❌                |
| video/H265      | ❌                |
| text/html       | ❌                |
| text/plain      | ❌                |
| text/xml        | ❌                |
| text/rtf        | ❌                |
| application/pdf | ❌                |

#### Thumbnail Resources

| Description      | Path                                                                             | Support Status |
| ---------------- | -------------------------------------------------------------------------------- | :------------: |
| Study Instances  | `studies/{study}/thumbnail`                                                      |       ✅       |
| Series Instances | `studies/{study}/series/{series}/thumbnail`                                      |       ✅       |
| Instance         | `studies/{study}/series/{series}/instances/{instance}/thumbnail`                 |       ✅       |
| Frames           | `studies/{study}/series/{series}/instances/{instance}/frames/{frames}/thumbnail` |       ❌       |

The thumbnail resources just perform a 303 redirect to their rendered counterparts

#### Bulkdata Resources

❌ Bulkdata Resources are not supported.

#### Pixel Data Resources

❌ Pixel Data Resources are not supported.

### Search for DICOM objects (QIDO-RS)

https://www.dicomstandard.org/using/dicomweb/query-qido-rs

#### Resources

| Resource                  | URI Template                                           | Support Status |
| ------------------------- | ------------------------------------------------------ | :------------: |
| All Studies               | `/studies{?search*}`                                   |       ✅       |
| Study's Series            | `/studies/{study}/series{?search*}`                    |       ✅       |
| Study's Series' Instances | `/studies/{study}/series/{series}/instances{?search*}` |       ✅       |
| Study's Instances         | `/studies/{study}/instances{?search*}`                 |       ✅       |
| All Series                | `/series{?search*}`                                    |       ✅       |
| All Instances             | `/instances{?search*}`                                 |       ✅       |

#### Query Parameters

| Key           | Description                             | Support Status |
| ------------- | --------------------------------------- | :------------: |
| {attributeID} | Query matching on supplied value        |       ✅       |
| includefield  | Include supplied tags in result         |       ✅       |
| fuzzymatching | Whether query should use fuzzy matching |       ❌       |
| limit         | Return only {n} results                 |       ✅       |
| offset        | Skip {n} results                        |       ✅       |

### Store DICOM objects (STOW-RS)

https://www.dicomstandard.org/using/dicomweb/store-stow-rs

#### Resources

| Resource | URI Template       | Support Status |
| -------- | ------------------ | :------------: |
| Studies  | `/studies`         |       ✅       |
| Study    | `/studies/{study}` |       ❌       |

### Search for Modality Worklist (MWL-RS)

https://www.dicomstandard.org/news-dir/current/docs/sups/sup246.pdf

#### Resources

| Resource       | URI Template                                    | Support Status |
| -------------- | ----------------------------------------------- | :------------: |
| Worklist Items | `/modality-scheduled-procedure-steps{?search*}` |       ✅       |

#### Query Parameters

| Key           | Description                             | Support Status |
| ------------- | --------------------------------------- | :------------: |
| {attributeID} | Query matching on supplied value        |       ✅       |
| includefield  | Include supplied tags in result         |       ✅       |
| fuzzymatching | Whether query should use fuzzy matching |       ❌       |
| limit         | Return only {n} results                 |       ✅       |
| offset        | Skip {n} results                        |       ✅       |

### Manage worklist items (UPS-RS)

https://www.dicomstandard.org/using/dicomweb/workflow-ups-rs

❌ UPS-RS is not supported.

## DICOM-RST Features

DICOM-RST provides additional features that are not part of the DICOMweb specification.

### AET list

Returns a list of configured AETs.

| Resource | URI Template |
| -------- | ------------ |
| AET List | `/aets`      |

### Health Check

Returns a simple OK if the connection is still healthy.

| Resource     | URI Template  |
| ------------ | ------------- |
| Health Check | `/aets/{aet}` |

### Dynamic AE Registration

Registers and removes AEs at runtime. Disabled by default.
Registered AEs are kept in memory only and are lost after a restart.

| Resource    | Method   | URI Template  |
| ----------- | -------- | ------------- |
| Register AE | `POST`   | `/aets`       |
| Remove AE   | `DELETE` | `/aets/{aet}` |

Requests must send a bearer token, and AEs may only point to allowed hosts.
Allowed hosts can be IP addresses, CIDR ranges or hostnames, optionally restricted to a single port:

```yaml
server:
  dynamic-registration:
    enabled: true
    token-env: DICOM_RST_ADMIN_TOKEN # environment variable that contains the bearer token
    allowed-hosts:
      - host: 10.0.0.0/8 # any address from 10.0.0.0 to 10.255.255.255
      - host: 192.168.10.0/24 # any address from 192.168.10.0 to 192.168.10.255, but only port 104
        port: 104
      - host: fd00::/8 # IPv6 unique local addresses
      - host: pacs.internal # exact hostname
```

To allow any host, use `0.0.0.0/0` and `::/0`.
Each AET can only be registered once. Registering it again with the same configuration has no effect,
a different configuration is rejected with `409 Conflict`.
See the [configuration documentation](docs/topics/configuration.md) for details.

[^1]:
    The [DICOM-RST logo](./dicom-rst-icon.png) is adapted from
    the [Rust logo](https://github.com/rust-lang/rust-artwork)
    owned by the Rust Foundation, used under CC-BY.
