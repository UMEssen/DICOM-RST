<div align="center">

![DICOM-RST Logo[^1]](./dicom-rst-icon.png)

# DICOM-RST

**A robust DICOMweb server with interchangeable backends**

Developed as part of the [Open Medical Inference](https://diz.ikim.nrw/en/project/omi/) methodology platform.

[Changelog](./CHANGELOG.md) | [Wiki](https://github.com/UMEssen/DICOM-RST/wiki)

</div>

---

> [!WARNING]  
> This project is highly experimental.
>
> We're still gathering information about potential compatibility issues with various PACS vendors.

DICOM-RST implements a DICOMweb-compatible HTTP server with support for QIDO-RS, WADO-RS and STOW-RS.

Currently, only the DIMSE backend is implemented, which translates DICOMweb requests into DIMSE-C operations.

## DICOMweb Features

Actual support may vary depending on the features implemented by the origin server.

### Retrieve DICOM objects (WADO-RS)

https://www.dicomstandard.org/using/dicomweb/retrieve-wado-rs-and-wado-uri

#### Instance Resources

| Description      | Path                                                   | Support Status |
|------------------|--------------------------------------------------------|:--------------:|
| Study Instances  | `studies/{study}`                                      |       ✅        |
| Series Instances | `studies/{study}/series/{series}`                      |       ✅        |
| Instance         | `studies/{study}/series/{series}/instances/{instance}` |       ✅        |

#### Metadata Resources

❌ Metadata Resources are not supported.

#### Rendered Resources

❌ Rendered Resourced are not supported.

#### Thumbnail Resources

❌ Thumbnail Resources are not supported.

#### Bulkdata Resources

❌ Bulkdata Resources are not supported.

#### Pixel Data Resources

❌ Pixel Data Resources are not supported.

### Search for DICOM objects (QIDO-RS)

https://www.dicomstandard.org/using/dicomweb/query-qido-rs

#### Resources

| Resource                  | URI Template                                           | Support Status |
|---------------------------|--------------------------------------------------------|:--------------:|
| All Studies               | `/studies{?search*}`                                   |       ✅        |
| Study's Series            | `/studies/{study}/series{?search*}`                    |       ✅        |
| Study's Series' Instances | `/studies/{study}/series/{series}/instances{?search*}` |       ✅        |
| Study's Instances         | `/study/{study}/instances{?search*}`                   |       ✅        |
| All Series                | `/series{?search*}`                                    |       ✅        |
| All Instances             | `/instances{?search*}`                                 |       ✅        |

#### Query Parameters

| Key           | Description                             | Support Status |
|---------------|-----------------------------------------|:--------------:|
| {attributeID} | Query matching on supplied value        |       ✅        |
| includefield  | Include supplied tags in result         |       ✅        |
| fuzzymatching | Whether query should use fuzzy matching |       ❌        |
| limit         | Return only {n} results                 |       ✅        |
| offset        | Skip {n} results                        |       ✅        |

### Store DICOM objects (STOW-RS)

https://www.dicomstandard.org/using/dicomweb/store-stow-rs

#### Resources

| Resource | URI Template       | Support Status |
|----------|--------------------|:--------------:|
| Studies  | `/studies`         |       ✅        |
| Study    | `/studies/{study}` |       ❌        |

### Manage worklist items (UPS-RS)

https://www.dicomstandard.org/using/dicomweb/workflow-ups-rs

❌ UPS-RS is not supported.

## DICOM-RST Features

DICOM-RST provides additional features that are not part of the DICOMweb specification.

### AET list

Returns a list of configured AETs.

| Resource | URI Template |
|----------|--------------|
| AET List | `/aets`      |

### Health Check

Returns a simple OK if the connection is still healthy.

| Resource     | URI Template   |
|--------------|----------------|
| Health Check | `/aets/{aets}` |

## Deployment

### Docker

```shell
# Build the Docker image...
docker build -t dicom-rst .

# ...and run it!
docker run dicom-rst
```

### Cargo

Cargo makes it easy to build from source:

```shell
cargo install --git https://github.com/UMEssen/DICOM-RST dicom-rst
``` 

#### Crate Features

- `dimse` (default): Enables the DIMSE backend
- `s3`: Enables the S3 backend

> [!TIP]
> If the DIMSE backend is not needed, it can be removed by using the `--no-default-features` option.

[^1]: The [DICOM-RST logo](./dicom-rst-icon.png) is adapted from
the [Rust logo](https://github.com/rust-lang/rust-artwork)
owned by the Rust Foundation, used under CC-BY.