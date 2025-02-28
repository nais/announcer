FROM cgr.dev/chainguard/rust AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM cgr.dev/chainguard/glibc-dynamic
COPY --from=build --chown=nonroot:nonroot /app/target/release/announcements /usr/local/bin/announcements
CMD ["/usr/local/bin/announcements"]
