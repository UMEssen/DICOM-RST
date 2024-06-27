# Troubleshooting

When you encounter an issue, always make sure that you are using the latest version of %product%.
Also check the logs for errors or warnings.
Setting the log level to `DEBUG` or even `TRACE` can provide additional information for troubleshooting.

As %product% is still in early development, we're still identifying compatibility issues and general bugs.
Don't hesitate to submit new issues to <a href="https://github.com/UMEssen/DICOM-RST/issues">GitHub issues</a> if you
think that something doesn't work as expected.

## Transfers are slow

When building from source, make sure that you're using the `--release` option to build an optimized release.
The Docker image already uses an optimized build.

If you think that the slow transfer is not due to network conditions,
you can use Sentry to analyze the application traces.
We're strongly recommending a [self-hosted Sentry](https://develop.sentry.dev/self-hosted/) instance.
It will show the execution time for each function, making it easy to identify possible bottlenecks in the application.

## WADO-RS returns no data

### DIMSE Backend

- Verify that the `wado-rs.receiver` option in the config file points to a valid DIMSE server (`server.dimse.aet`).
- Check if the Search Service (QIDO-RS) returns data. If it doesn't return search results, it's probably a general
  connection issue that will affect all other DICOMweb services as well.
- Verify that the called AET accepts messages from the calling AET (`server.aet`) and is allowed to send to the
  STORE-SCP (`server.dimse.aet`). Usually a combination of IP, port and AET needs to be whitelisted.
- Use the `server.dimse.uncompressed` option. It was observed that some legacy PACS fail to send compressed instances (
  internal exceptions, even though the transfer syntax is supported and accepted).

### S3 Backend

- Verify that the folder structure follows the hierarchy explained in the [S3 backend](backend-s3.md) chapter.

- Check your credentials (`access-key`, `secret-key`)
