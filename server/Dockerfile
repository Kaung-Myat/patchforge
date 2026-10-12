FROM rust:1.98-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --locked --release --bin server

FROM debian:bookworm-slim
WORKDIR /app/server
COPY --from=builder /build/target/release/server ./server
USER 65532:65532
EXPOSE 8080
CMD ["./server"]
