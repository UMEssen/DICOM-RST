# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

DICOM-RST is a DICOMweb-compatible gateway server written in Rust. It translates DICOMweb HTTP requests (QIDO-RS, WADO-RS, STOW-RS) to backend storage systems:

- **DIMSE backend**: Translates DICOMweb to DIMSE-C protocol (C-FIND, C-MOVE, C-STORE) for PACS communication
- **S3 backend** (feature-gated): Retrieves DICOM instances from S3-compatible storage
- **Plugin backend** (feature-gated): Dynamically loaded plugins for custom backend implementations

## Build Commands

```bash
# Build (default - DIMSE backend only)
cargo build

# Build with S3 backend support
cargo build --features s3

# Build with plugin support
cargo build --features plugins

# Build example plugin (produces .so/.dylib/.dll)
cargo build -p dicom-rst-example-plugin --release

# Run with development config
cargo run

# Run tests
cargo test

# Format code (uses hard tabs per .rustfmt.toml)
cargo fmt

# Lint (project uses pedantic, nursery, and cargo clippy lints)
cargo clippy --all-targets --all-features

# Dependency license/security check
cargo deny check
```

## Architecture

### Core Modules

- `src/main.rs` - Application entry point, Axum HTTP server setup, and DIMSE server spawning
- `src/api/` - HTTP route handlers organized by DICOMweb service (qido/, wado/, stow/)
- `src/backend/` - Backend implementations behind service traits
  - `backend/mod.rs` - `ServiceProvider` extractor that routes requests to the correct backend based on AET config
  - `backend/dimse/` - DIMSE protocol implementation with association pooling and DICOM SCU/SCP
  - `backend/s3/` - S3 backend (requires `s3` feature)
  - `backend/plugin/` - Plugin loading and adapters (requires `plugins` feature)
- `dicom-rst-plugin-api/` - Plugin API crate for external plugin development
- `example-plugin/` - Example plugin demonstrating the plugin interface
- `src/config/` - Configuration loading from YAML files and environment variables
- `src/rendering/` - DICOM image rendering for `/rendered` endpoints

### Key Design Patterns

**Backend Abstraction**: Services are defined as traits (`QidoService`, `WadoService`, `StowService`) in `src/api/{qido,wado,stow}/service.rs`. The `ServiceProvider` in `src/backend/mod.rs` is an Axum extractor that instantiates the appropriate backend based on the AET path parameter.

**Association Pooling**: DIMSE backends use `AssociationPools` for connection reuse to PACS systems.

**Move Mediator**: For WADO-RS, the `MoveMediator` coordinates C-MOVE operations where DICOM-RST acts as both the SCU (requesting move) and SCP (receiving instances).

### Configuration

Configuration loads from (in order):
1. `src/config/defaults.yaml` - embedded defaults
2. `config.yaml` - optional file in working directory
3. Environment variables prefixed with `DICOM_RST_`

Each AET entry defines which backend to use and service-specific timeouts.

## Code Style

- Uses hard tabs for indentation
- Clippy is run with `pedantic`, `nursery`, and `cargo` lint groups enabled
- `unsafe_code` is forbidden

## Plugin Development

### Overview

Plugins allow external implementations of DICOMweb services (QIDO-RS, WADO-RS, STOW-RS). Plugins are shared libraries (`.so`, `.dylib`, `.dll`) loaded at runtime using `abi_stable` for C ABI compatibility across Rust versions.

### Crate Structure

```
dicom-rst-plugin-api/     # Plugin API - depend on this to create plugins
├── src/
│   ├── lib.rs            # PluginModule, declare_plugin! macro
│   ├── types.rs          # FFI-safe types (FfiSearchRequest, FfiError, etc.)
│   ├── streaming.rs      # FFI-safe streaming traits
│   ├── qido.rs           # QidoPlugin trait
│   ├── wado.rs           # WadoPlugin trait
│   └── stow.rs           # StowPlugin trait
```

### Creating a Plugin

1. Create a new library crate with `crate-type = ["cdylib"]`
2. Depend on `dicom-rst-plugin-api`
3. Implement the plugin traits (`QidoPlugin`, `WadoPlugin`, `StowPlugin`)
4. Use `declare_plugin!` macro to export the module

```rust
use dicom_rst_plugin_api::*;

struct MyQidoPlugin;

impl QidoPlugin for MyQidoPlugin {
    fn search(&self, request: FfiSearchRequest)
        -> FfiFuture<FfiResult<FfiDicomObjectStreamBox>>
    {
        FfiFuture::new(async {
            // Query your backend, return stream of DICOM JSON objects
            FfiResult::ROk(create_stream())
        })
    }

    fn health_check(&self) -> FfiFuture<FfiResult<()>> {
        FfiFuture::new(async { FfiResult::ROk(()) })
    }
}

declare_plugin! {
    plugin_id: "my-plugin",
    version: env!("CARGO_PKG_VERSION"),
    capabilities: PluginCapabilities::qido_only(),
    initialize: |config| { /* parse config.config_json */ FfiResult::ROk(()) },
    create_qido: || ROption::RSome(QidoPlugin_TO::from_value(MyQidoPlugin, TD_Opaque)),
    create_wado: || ROption::RNone,
    create_stow: || ROption::RNone,
}
```

### Plugin Configuration

Plugins are configured in `config.yaml`:

```yaml
plugins:
  - path: "/path/to/libmy_plugin.so"
    aets:
      - MY_PACS
      - ANOTHER_AET
    settings:
      database_url: "postgres://localhost/dicom"
      custom_option: true
```

- `path`: Path to the shared library
- `aets`: List of AETs this plugin handles (requests to these AETs go to the plugin)
- `settings`: Arbitrary JSON passed to the plugin's `initialize` function

### Key Types

| Type | Purpose |
|------|---------|
| `FfiSearchRequest` | QIDO search parameters (level, UIDs, match criteria) |
| `FfiRetrieveRequest` | WADO retrieve parameters (resource query) |
| `FfiStoreRequest` | STOW store parameters (list of DICOM files) |
| `FfiDicomObject` | DICOM object as JSON string |
| `FfiDicomFile` | Raw DICOM Part 10 bytes |
| `FfiError` | Error with code and message |
| `FfiDicomObjectStreamBox` | Async stream of DICOM objects |
| `FfiDicomFileStreamBox` | Async stream of DICOM files |

### Plugin Loading

1. Plugins are loaded at application startup from the `plugins` config
2. The `PluginRegistry` (`src/backend/plugin/registry.rs`) manages loaded plugins
3. `ServiceProvider` checks the plugin registry before falling back to built-in backends
4. Adapter classes (`PluginQidoAdapter`, etc.) bridge FFI types to internal service traits
