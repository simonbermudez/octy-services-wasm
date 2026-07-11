FROM rust:1-slim AS build
WORKDIR /src
COPY . .
RUN cargo build -p octy-data-gateway --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/octy-data-gateway /usr/local/bin/octy-data-gateway
EXPOSE 8090
ENTRYPOINT ["octy-data-gateway"]
