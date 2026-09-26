# Configuration

%product% requires configuration before it can run in your environment.

This configuration will be loaded from a file named `config.yaml` next to the binary.

When using Docker, you can use [volumes](https://docs.docker.com/storage/volumes/) to mount a local config file into the
container.
In the container, the config file is always located in the root directory.
The `:ro` option will mount the file into the container as read-only.

<tabs>
    <tab id="docker-compose" title="Docker Compose">
        <code-block lang="yaml">
            services:
              dicom-rst:
                image: ghcr.io/umessen/dicom-rst:0.3.0
                ports:
                  - "8080:8080"
                  - "7001:7001"
                volumes:
                  - ./dicom-rst.yaml:/config.yaml:ro
        </code-block>
    </tab>
    <tab id="docker-run" title="Docker Run">
        <code-block lang="shell">
            docker run \
            -p 8080:8080 \
            -p 7001:7001 \
            -v ./dicom-rst.yaml:/config.yaml:ro \ 
            ghcr.io/umessen/dicom-rst:latest
        </code-block>
    </tab>

</tabs>

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
    interface: 0.0.0.0
    port: 8080
    max-upload-size: 50000000
    request-timeout: 60000
    graceful-shutdown: true
  dimse:
    - aet: DICOM-RST
      interface: 0.0.0.0
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
  audit:
    enabled: false
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
    <def title="telemetry.audit" id="telemetry.audit">
        Structured access-audit logging, disabled by default.
        See <a href="#access-audit-config">Access Audit Config</a>.
    </def>
</deflist>

## Access Audit Config {id="access-audit-config"}

%product% performs no authentication itself. When it runs behind an authenticating reverse proxy such as
<a href="https://oauth2-proxy.github.io/oauth2-proxy/">oauth2-proxy</a>, it can write one access-audit record per
HTTP request, naming the user the proxy verified and the DICOM resources that were accessed.
All settings are optional; with <code>enabled: false</code> (the default) nothing changes.

```yaml
telemetry:
  audit:
    enabled: true
    user-header: X-Forwarded-Email
    subject-header: X-Forwarded-User
    trusted-relays: []
    on-behalf-of-header: X-On-Behalf-Of
```

<deflist>
    <def title="telemetry.audit.enabled" id="telemetry.audit.enabled">
        Enables the access-audit log (default <code>false</code>).
        Every HTTP request then emits one self-contained JSON line on stdout.
        Delivery is fail-open: records pass through a bounded buffer, so a slow log consumer never blocks
        requests; dropped records are counted and logged as warnings.
    </def>
    <def title="telemetry.audit.user-header" id="telemetry.audit.user-header">
        The request header carrying the user verified by the proxy, recorded as <code>user</code>
        (default <code>X-Forwarded-Email</code>).
        This is also the identity that <code>trusted-relays</code> is matched against.
    </def>
    <def title="telemetry.audit.subject-header" id="telemetry.audit.subject-header">
        The request header carrying the subject identifier verified by the proxy, recorded as <code>subject</code>
        (default <code>X-Forwarded-User</code>).
    </def>
    <def title="telemetry.audit.trusted-relays" id="telemetry.audit.trusted-relays">
        Identities, exactly as the proxy asserts them in <code>user-header</code>, that may name the end user they act
        for (default: empty, nobody may).
        Use this for services that call %product% with their own credentials on behalf of a signed-in user.
        Matching is ASCII case-insensitive. Empty entries are rejected at startup.
    </def>
    <def title="telemetry.audit.on-behalf-of-header" id="telemetry.audit.on-behalf-of-header">
        The request header in which a trusted relay names the end user (default <code>X-On-Behalf-Of</code>).
        It is only read when <code>trusted-relays</code> is not empty, and must differ from
        <code>user-header</code> and <code>subject-header</code>.
    </def>
</deflist>

Header names are validated when the configuration is loaded; an invalid name, or a header that carries credentials
(<code>Authorization</code>, <code>Proxy-Authorization</code>, <code>Cookie</code>), stops %product% at startup with an
error naming the offending key.

### Audit Record

```json
{"audit":"http-access","ts":"2026-08-17T17:16:55Z","user":"jane.doe@example.org","subject":"3f2c9a4e","source":"192.0.2.10","method":"GET","path":"/aets/PACS/studies/1.2.3.4","aet":"PACS","study":"1.2.3.4","status":200,"duration_ms":4886,"request_id":"0f8c2b7e"}
```

| Field                   | Description                                                                               |
|-------------------------|-------------------------------------------------------------------------------------------|
| `audit`                 | Always `http-access`.                                                                     |
| `ts`                    | Completion time (UTC, RFC 3339, second precision).                                        |
| `user`, `subject`       | Values of `user-header` and `subject-header`: 1 to 320 bytes of UTF-8 without control characters. Omitted if absent, sent more than once or malformed (never truncated). |
| `on_behalf_of`          | The end user named by a trusted relay (see below).                                        |
| `on_behalf_of_rejected` | Why an on-behalf-of header was ignored: `untrusted-caller` or `invalid`.                  |
| `source`                | First entry of `X-Forwarded-For`, at most 64 bytes.                                       |
| `method`, `path`        | Request method, and path with query string (at most 8 KiB).                               |
| `aet`, `study`, `series`, `instance` | DICOM coordinates from the request path.                                     |
| `status`, `duration_ms` | Response status (including `408` for timed-out requests) and duration.                    |
| `user_agent`            | `User-Agent` header, at most 512 bytes.                                                   |
| `request_id`            | `X-Request-Id` header, if sent once with 1 to 128 visible ASCII characters.               |

Absent values are omitted. Values longer than their limit are cut at a character boundary and end in `…`. With auditing enabled, the log line of a completed C-MOVE also carries the
<code>study_uid</code>.

### Trust Model

The identity fields are only as trustworthy as the proxy in front of %product%:

<warning>
    <list>
        <li>The proxy must remove any <code>user-header</code>, <code>subject-header</code> and
        <code>on-behalf-of-header</code> a client sends and set the identity headers itself from the verified
        session. oauth2-proxy does this for <code>X-Forwarded-User</code> and <code>X-Forwarded-Email</code> with
        <code>pass_user_headers</code> (enabled by default). The <code>X-Auth-Request-*</code> headers are response
        headers in reverse-proxy mode and must not be used.</li>
        <li>The proxy must be the only way to reach %product%'s HTTP port, for example by binding
        <code>server.http.interface</code> to <code>127.0.0.1</code> next to a sidecar proxy, or with a firewall or
        network policy. Anyone who can reach the port directly can send any header.</li>
    </list>
</warning>

A relay is trusted because of the identity the proxy verified for it, never because of a header it sends itself.
An on-behalf-of claim is recorded as <code>on_behalf_of</code> only if the request's <code>user</code> is listed in
<code>trusted-relays</code>, the header occurs exactly once, and its value is 1 to 320 bytes of UTF-8 without
whitespace or control characters. Otherwise the record carries <code>on_behalf_of_rejected</code>
(<code>untrusted-caller</code> or <code>invalid</code>) and the claimed value is not logged.
The relay is responsible for naming the correct end user; %product% records the claim, it cannot verify it.

### Example: oauth2-proxy

An oauth2-proxy configuration (excerpt) in front of %product%, accepting both interactive users and services that
present their own bearer token:

```toml
provider = "oidc"
oidc_issuer_url = "https://idp.example.org"
upstreams = ["http://127.0.0.1:8080/"]
email_domains = ["*"]
# Default: sets X-Forwarded-User/-Email from the session, replacing client copies
pass_user_headers = true
# Lets services call with a bearer token issued by the same provider
skip_jwt_bearer_tokens = true
```

For a service, the proxy fills the identity headers from the claims of its token, so with the default
<code>user-header</code> the relay's token needs an e-mail claim.
With the matching %product% configuration, requests from <code>viewer@example.org</code> may name the end user in
<code>X-On-Behalf-Of</code>:

```yaml
server:
  http:
    interface: 127.0.0.1
telemetry:
  audit:
    enabled: true
    trusted-relays:
      - viewer@example.org
```

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
    interface: 0.0.0.0
    port: 8080
    max-upload-size: 50000000
    request-timeout: 60000
    graceful-shutdown: true
    base-path: /
```

<deflist>
    <def title="server.http.interface" id="server.http.interface">
        The interface the HTTP server should bind to. Uses <code>0.0.0.0</code> by default.
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
    <def title="server.http.base-path" id="server.http.base-path">
        Sets the base path for all HTTP endpoints.
    </def>
</deflist>

## DIMSE Server Config

When using the DIMSE backend, a running STORE-SCP is required to receive incoming DICOM instances.
You can spawn multiple DIMSE server that will act as the STORE-SCP.

```yaml
server:
  dimse:
    - aet: MY-STORE-SCP
      interface: 0.0.0.0
      port: 7001
      uncompressed: true
```

<deflist>
    <def title="server.dimse.aet" id="server.dimse.aet">
    The AET for this DIMSE server. Make sure that this AET is whitelisted and is a valid destination for C-MOVEs.
    </def>
    <def title="server.dimse.interface" id="server.dimse.interface">
    The interface this DIMSE server should bind to.
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
    endpoint: http://dicom.s3.local
    endpoint-style: vhost
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
    <def title="endpoint-style" id="s3.endpoint-style">
        Sets the URL access style. Defaults to <b>vhost</b>.
        Allowed values are:
        <list>
        <li><b>path</b>: For the deprecated <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/VirtualHosting.html#path-style-access">path-style</a>.</li>
        <li><b>vhost</b>: For the <a href="https://docs.aws.amazon.com/AmazonS3/latest/userguide/VirtualHosting.html#virtual-hosted-style-access">virtual-hosted-style</a>.</li>
        </list>
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