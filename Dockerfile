FROM lukemathwalker/cargo-chef:latest-rust-1.93 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json

RUN cargo chef cook --release --features mimalloc --recipe-path recipe.json

COPY . .
RUN cargo build --release --features mimalloc --bin dicom-rst

FROM gcr.io/distroless/cc-debian12 AS runtime
COPY --from=builder /app/target/release/dicom-rst /
ENTRYPOINT ["./dicom-rst"]
