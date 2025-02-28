ARG PACKAGE=announcements

FROM cgr.dev/chainguard/rust as build
WORKDIR /app
COPY cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM cgr.dev/chainguard/glibc-dynamic
COPY --from=build --chown=nonroot:nonroot /app/target/release/${PACKAGE} /usr/local/bin/${PACKAGE}
CMD ["/usr/local/bin/${PACKAGE}"]
