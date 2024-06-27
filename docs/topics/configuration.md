# Configuration

%product% requires configuration before it can run in your environment.

This configuration will be loaded from a file named `config.yaml` next to the binary.

## Example Config

The following configuration provides all relevant settings.
As a starting point, you can copy this configuration and adapt it to your needs.

```yaml
telemetry:
  sentry: https://sentry.local/dsn
  level: INFO
server:
  aet: DICOM-RST
  http:
    host: 0.0.0.0
    port: 8080
    max-upload-size: 50000000
    request-timeout: 60000
    graceful-shutdown: true
  dimse:
    - aet: DICOM-RST
      host: 0.0.0.0
      port: 7001
      uncompressed: true
aets:
  - aet: MY-PACS
    host: 127.0.0.1
    port: 4242
    backend: DIMSE
    pool:
      size: 16
      timeout: 10000
    qido-rs:
      timeout: 10000
    stow-rs:
      timeout: 10000
    wado-rs:
      timeout: 30000
      mode: concurrent
      receivers:
        - DICOM-RST # see server.dimse.aet
```

## Telemetry Config

```yaml
telemetry:
  sentry: https://sentry.local/dsn
  level: INFO
```

<deflist>
    <def title="telemetry.sentry">
        The <a href="https://docs.sentry.io/concepts/key-terms/dsn-explainer/">Sentry DSN</a>. 
        If empty or not present, this will disable Sentry.
    </def>
    <def title="telemetry.level">
        The logging level. Possible values are (sorted by verbosity): 
        <list>
          <li>ERROR</li>
          <li>WARN</li>
          <li>INFO</li>
          <li>DEBUG</li>
          <li>TRACE</li>
        </list>
    </def>
</deflist>

## Global Server Config

```yaml
server:
  aet: DICOM-RST
```

<deflist>
    <def title="server.aet">
        The AET that should be used by %product% as the calling AET. 
        When using the DIMSE backend, make sure that this AET is whitelisted by the called AET. 
    </def>
</deflist>

## HTTP Server Config

```yaml
server:
  http:
    host: 0.0.0.0
    port: 8080
    max-upload-size: 50000000
    request-timeout: 60000
    graceful-shutdown: true
```

<deflist>
    <def title="server.http.host" id="server.http.host">
        The host address of the server, Uses <code>0.0.0.0</code> by default.
    </def>
    <def title="server.http.port" id="server.http.port">
        The port for the HTTP server. Uses <code>8080</code> by default.
    </def>
    <def title="server.http.max-upload-size" id="server.http.max-upload-size">
        The maximum allowed request body size in bytes. 
        This setting can be used to limit the upload size of STOW-RS requests.
    </def>
    <def title="server.http.request-timeout" id="server.http.request.timeout">
        The maximum allowed (total) time for a HTTP request in milliseconds before a timeout occurs. This applies to all endpoints.
    </def>
    <def title="server.http.graceful-shutdown" id="server.http.graceful-shutdown">
        If enabled, %product% will wait for outstanding requests to complete before stopping the process.
        Shutdowns will take no longer than the request timeout configured by <code>server.http.request.timeout</code>.
        If disabled, the process is stopped immediately, which will potentially lead to incomplete responses for outstanding requests.
    </def>
</deflist>

## DIMSE Server Config

When using the DIMSE backend, a running STORE-SCP is required to receive incoming DICOM instances.
You can spawn multiple DIMSE server that will act as the STORE-SCP.

```yaml
server:
  dimse:
    - aet: MY-STORE-SCP
      host: 0.0.0.0
      port: 7001
      uncompressed: true
```

<deflist>
    <def title="server.dimse.aet" id="server.dimse.aet">
    The AET for this DIMSE server. Make sure that this AET is whitelisted and is a valid destination for C-MOVEs.
    </def>
    <def title="server.dimse.host" id="server.dimse.host">
    The host address for this DIMSE server.
    </def>
    <def title="server.dimse.port" id="server.dimse.port">
    The port for this DIMSE server.
    </def>
    <def title="server.dimse.uncompressed" id="server.dimse.uncompressed">
    If enabled, the DIMSE server will propose uncompressed transfer syntaxes only.
    We've encountered some PACS that will happily accept compressed transfer syntaxes, 
    but throw an internal exception when compressing before sending it to the STORE-SCP.
    Enforcing a uncompressed transfer syntax will fix this.
    </def>

</deflist>

## DICOMweb Config

```yaml
aets:
  - aet: EXAMPLE
    backend: DIMSE
    # <…>
    qido-rs:
      timeout: 3000
    wado-rs:
      timeout: 3000
    stow-rs:
      timeout: 3000
```

Each AET (regardless of the backend) has additional settings specific to the DICOMweb endpoints.

<deflist>
    <def title="qido-rs.timeout" id="dicomweb.qido-rs.timeout">
    How many milliseconds to wait until a QIDO-RS request should time out.
    This is the timeout for a single operation (e.g. receiving a DIMSE-C response primitive).
    If you want to set a timeout for the total execution time, use the <code>server.http.request-timeout</code> option instead.
    </def>
    <def title="wado-rs.timeout" id="dicomweb.wado-rs.timeout">
    How many milliseconds to wait until a WADO-RS request should time out.
    This is the timeout for a single operation (e.g. receiving a DIMSE-C response primitive).
    If you want to set a timeout for the total execution time, use the <code>server.http.request-timeout</code> option instead.
    Consult the conformance statement of your PACS to see how often a C-MOVE-RSP is returned and adapt this option accordingly.
    If a C-MOVE-RSP is returned every 20 seconds, set this to 30 seconds (20s + 10s leeway) for example.
    </def>
    <def title="wado-rs.mode" id="dicomweb.wado-rs.mode">
    <b>DIMSE-backend only:</b>
    Some PACS do not include the <code>MoveOriginatorMessageId</code> attribute in their C-STORE-RQ messages.
    This makes it hard to assign incoming C-STORE-RQ responses to an active C-MOVE operation.
    As a workaround, you can set the receive mode to <code>sequential</code> to disable concurrent C-MOVEs.
    Throughput will be limited, but it will work reliably. Consider increasing the timeouts when using the <b>sequential</b> mode.
    Most PACS can and should use the <b>concurrent</b> mode.
    <list>
        <li><b>concurrent</b>: C-MOVE requests are processed concurrently.</li>
        <li><b>sequential</b>: C-MOVE requests are processed sequentially.</li>
    </list>
    </def>
    <def title="wado-rs.receivers" id="dicomweb.wado-rs.receivers">
    <b>DIMSE-backend only:</b>
    A list of AETs for STORE-SCPs that can act as the receiver for this AET.
    The AET must match with a value from <code>server.dimse.aet</code>.
    </def>
    <def title="stow-rs.timeout" id="dicomweb.stow-rs.timeout">
    How many milliseconds to wait until a STOW-RS request should time out.
    This is the timeout for a single operation (e.g. receiving a DIMSE-C response primitive).
    If you want to set a timeout for the total execution time, use the <code>server.http.request-timeout</code> option instead.
    </def>
</deflist>

## S3 Backend Config

The following options are available if the S3 backend is selected:

```yaml
aets:
  - aet: RESEARCH
    backend: S3
    endpoint: http://s3.local
    bucket: research
    region: local
    concurrency: 32
    credentials:
        access-key: ABC123
        secret-key: topSecret
```

<deflist>
    <def title="endpoint" id="s3.endpoint">
    The endpoint where the S3 compatible storage is available
    </def>
    <def title="bucket" id="s3.bucket">
    The bucket to access. We recommend this to be the same as the AET.
    </def>
    <def title="region" id="s3.region">
    The region of the S3 bucket. Set this to a dummy value (like "local") if this is not required. 
    Most S3 implementations require a region, even though it doesn't make sense for on-premise deployments.
    </def>
    <def title="credentials.access-key" id="s3.credentials.access-key">
    The access key for S3 authentication. For anonymous access, remove the entire <code>credentials</code> object from your config. 
    </def>
    <def title="credentials.secret-key" id="s3.credentials.secret-key">
    The secret key for S3 authentication. For anonymous access, remove the entire <code>credentials</code> object from your config.
    </def>
    <def title="concurrency" id="s3.concurrency">
    The maximum allowed amount of concurrent S3 operations.
    Increasing the amount of concurrency will massively improve throughput.
    </def>
</deflist>

## DIMSE Backend Config

The following options are available if the DIMSE backend is selected:

```yaml
aets:
  - aet: RESEARCH
    backend: DIMSE
    host: 127.0.0.1
    port: 11112
    pool:
      size: 32
      timeout: 30000
```

<deflist>
    <def title="host" id="dimse.host">
    The host address (IPv4 or IPv6) of the external AET that should be called by %product%. 
    </def>
    <def title="port" id="dimse.port">
    The port of the external AET that should be called by %product%.
    </def>
    <def title="pool.size" id="dimse.pool.size">
    The size of the connection pool. This sets the amount of concurrency, as there will be at most <code>pool.size</code> concurrent connections.
    </def>
    <def title="pool.timeout" id="dimse.pool.timeout">
    The maximum time (in milliseconds) to wait for a new connection from the pool.
    If the pool size is 2 and there are already 2 active connections, a third request will wait until one of the currently active connections is returned to the pool.
    </def>
</deflist>