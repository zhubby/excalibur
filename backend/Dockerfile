FROM rust:1.95-bookworm AS builder
WORKDIR /app
COPY backend ./backend
WORKDIR /app/backend
ARG BIN=excalibur-api
RUN cargo build --release --bin ${BIN}

FROM debian:bookworm-slim AS runtime
ARG BIN=excalibur-api
RUN useradd -m -u 10001 appuser
COPY --from=builder /app/backend/target/release/${BIN} /usr/local/bin/excalibur
USER appuser
EXPOSE 8080
CMD ["/usr/local/bin/excalibur"]

