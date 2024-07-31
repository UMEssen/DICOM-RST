# Installing DICOM-RST

There are multiple ways to get %product% up and running in your environment.

## Using Docker

%product% uses GitHub Actions to automate releases.
For each release, GitHub Actions will build and publish a Docker image that can be pulled from GitHub Packages.


<procedure title="Installing %product% using Docker" id="docker">
<step>
<p>Navigate to the <a href="https://github.com/UMEssen/DICOM-RST">repository page</a> and click on the <control>dicom-rst</control> package under <control>Packages</control> in the sidebar.
</p>
<img src="packages-dark.png" />
</step>

<step>
<p>
You'll see a list of published releases. Click on a version you would like to use and follow the instructions to install from the command line.
</p>
<img src="package-dark.png" />

</step>

<step>
<p>Pull the image as shown above:</p>
<code-block lang="shell">docker pull ghcr.io/umessen/dicom-rst:latest</code-block>
</step>
<step>
<p>Start the container:</p>
<code-block lang="shell">docker run -p 8080:8080 -p 7001:7001 ghcr.io/umessen/dicom-rst:latest</code-block>
<p>Make sure to expose the HTTP server (port 8080 by default) and the DIMSE server (port 7001 by default).</p>
</step>
</procedure>

## Building from source

It's also possible to build %product% from source using the Cargo build tool.

<procedure title="Installing %product% using Cargo" id="cargo">
    <step>
        <p>
            Download and install the Cargo build tool using <code>rustup</code>.
        </p>
        <code-block lang="shell">
        curl https://sh.rustup.rs -sSf | sh
        </code-block>
    </step>
    <step>
        <p>Build the <b>dicom-rst</b> crate with Cargo:</p>
        <code-block lang="shell">cargo install --git https://github.com/UMEssen/DICOM-RST dicom-rst</code-block>
    </step>
    <step>
        <p>Execute the built binary:</p>
        <code-block lang="shell">dicom-rst</code-block>
    </step>
</procedure>
