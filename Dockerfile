FROM cgr.dev/chainguard/rust AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM cgr.dev/chainguard/glibc-dynamic
COPY --from=build --chown=nonroot:nonroot /app/target/release/announcer /usr/local/bin/announcer
CMD ["/usr/local/bin/announcer"]
