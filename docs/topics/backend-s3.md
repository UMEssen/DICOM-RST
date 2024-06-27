# S3 Backend

The S3-Backend for DICOM-RST implements a subset of the DICOMweb standard by connecting to a S3-compatible storage to
retrieve DICOM instances.

It assumes the following folder structure:

```
{bucket}
- {study_instance_uid}
    - {series_instance_uid}
        - {sop_instance_uid}.dcm
        - {sop_instance_uid}.dcm
    - {series_instance_uid}
        - {sop_instance_uid}.dcm
- {study_instance_uid}
    - {series_instance_uid}
        - {sop_instance_uid}.dcm
        - {sop_instance_uid}.dcm
    - {series_instance_uid}
        - {sop_instance_uid}.dcm
```

Each bucket is named after the AET.
Within each bucket, study folders (named by StudyInstanceUID) are located at the root level.
These folders contain series folders (named by SeriesInstanceUID),
which in turn hold the individual DICOM instances (named by SOPInstanceUID) for the series.

## Query Service

Not implemented yet.

## Retrieve Service

[https://www.dicomstandard.org/using/dicomweb/retrieve-wado-rs-and-wado-uri](https://www.dicomstandard.org/using/dicomweb/retrieve-wado-rs-and-wado-uri)

### Requirements

The S3 backend requires a S3-compatible storage that implements
the [ListObjectsV2](https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListObjectsV2.html)
and [GetObject](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetObject.html) operations.

### Instance Resources

| Description      | Path                                                   | Support Status |
|------------------|--------------------------------------------------------|:--------------:|
| Study Instances  | `studies/{study}`                                      |       ✅        |
| Series Instances | `studies/{study}/series/{series}`                      |       ✅        |
| Instance         | `studies/{study}/series/{series}/instances/{instance}` |       ✅        |

### Metadata Resources

Metadata Resources are not supported.

### Rendered Resources

Rendered Resourced are not supported.

### Thumbnail Resources

Thumbnail Resources are not supported.

### Bulkdata Resources

Bulkdata Resources are not supported.

### Pixel Data Resources

Pixel Data Resources are not supported.

## Store Service

Not implemented yet.