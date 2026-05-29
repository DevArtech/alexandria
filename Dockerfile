# Build the alexandria-mcp server.
FROM rust:1-bookworm AS builder
WORKDIR /build
COPY . .
RUN cargo build --release --bin alexandria-mcp

# Minimal runtime image.
FROM debian:bookworm-slim
# ca-certificates: TLS to embedding endpoints. libgomp1/libstdc++6: only needed
# if you use the in-process fastembed embedder (ONNX Runtime). They are harmless
# otherwise; remove them if you exclusively use a remote/OpenAI-compatible embedder.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libgomp1 libstdc++6 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/alexandria-mcp /usr/local/bin/alexandria-mcp

# The memory library is a mounted volume; the index is rebuildable from it.
VOLUME ["/srv/alexandria"]
EXPOSE 8080

# Token is read from ALEXANDRIA_MCP_TOKEN at runtime (see docker-compose.yml).
ENTRYPOINT ["alexandria-mcp"]
CMD ["--transport", "http", "--bind", "0.0.0.0:8080", "--library", "/srv/alexandria"]
