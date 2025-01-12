FROM rust:1.84 AS chef
RUN cargo install --version 0.1.68 cargo-chef 
WORKDIR /app

FROM chef AS prepare
COPY . .
RUN cargo chef prepare  --recipe-path recipe.json

FROM chef AS build
COPY --from=prepare /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin fly-metrics-proxy

FROM debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update && apt-get install -y openssl ca-certificates
COPY --from=build /app/target/release/fly-metrics-proxy .

EXPOSE 8080
ENTRYPOINT ["/app/fly-metrics-proxy"]
