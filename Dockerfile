FROM rust:latest as build
WORKDIR /app
COPY . /app
WORKDIR /app/dicom-rst
RUN cargo build --release

FROM gcr.io/distroless/cc-debian12
COPY --from=build /app/target/release/dicom-rst /dicom-rst
# COPY --from=build /app/src/config/defaults.toml /config.toml
EXPOSE 3000
CMD ["/dicom-rst"]