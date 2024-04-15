# Configuration

The TOML format is used for config files.
See [defaults.toml](../src/config/defaults.toml) for an example.

## Telemetry

| Key                | Default | Description                                                                         |
|--------------------|---------|-------------------------------------------------------------------------------------|
| `telemetry.sentry` | None    | The DSN for a Sentry instance. A missing value or empty string will disable Sentry. |
| `telementry.level` | "INFO"  | The global log level.                                                               |

## Server

| Key          | Default     | Description                                                          |
|--------------|-------------|----------------------------------------------------------------------|
| `server.aet` | "DICOM-RST" | The Application Entity Title that identifies the DICOM-RST instance. |

## DIMSE Servers

| Key                        | Default     | Description                                                                                                                                                                               |
|----------------------------|-------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `server.dimse`             |             | An array of DIMSE server configurations.                                                                                                                                                  |
| `server.dimse.aet`         | "DICOM-RST" |                                                                                                                                                                                           |
| `server.dimse.host`        | "0.0.0.0"   |                                                                                                                                                                                           |
| `server.dimse.port`        | 7001        |                                                                                                                                                                                           |
| `server.dimse.notify-aets` | []          | A list of AETs to notify about received DICOM instances. This is required for WADO-RS using the DIMSE backend. Usually this should be set to the list of available AETs (`[[aets]].aet`). |

## HTTP Server

| Key                           | Default    | Description                                              |
|-------------------------------|------------|----------------------------------------------------------|
| `server.http.host`            | "0.0.0.0"  |                                                          |
| `server.http.port`            | 8080       |                                                          |
| `server.http.max-upload-size` | 50_000_000 | The maximum request body size in bytes.                  |
| `server.http.request-timeout` | 60_000     | The maximum time in milliseconds to wait for a response. |

## AETs

| Key                    | Default                                                          | Description                                                                     |
|------------------------|------------------------------------------------------------------|---------------------------------------------------------------------------------|
| `aets`                 |                                                                  | A list of AETs.                                                                 |
| `aets.aet`             |                                                                  | The title of the AE.                                                            |
| `aets.host`            |                                                                  |                                                                                 |
| `aets.port`            |                                                                  |                                                                                 |
| `aets.backend`         | "dimse" (with dimse-feature), "disabled" (without dimse-feature) | The backend that should be used to process DICOMweb requests for this AE.       |
| `aets.pool.size`       | 16                                                               | The size of the pool.                                                           |
| `aets.pool.timeout`    | 10_000                                                           | The maximum time in milliseconds to wait to acquire a connection from the pool. |
| `aets.qido-rs.timeout` | 10_000                                                           | Timeout in milliseconds for QIDO-RS requests                                    |
| `aets.wado-rs.timeout` | 30_000                                                           | Timeout in milliseconds for WADO-RS requests                                    |
| `aets.stow-rs.timeout` | 30_000                                                           | Timeout in milliseconds for STOW-RS requests                                    |



