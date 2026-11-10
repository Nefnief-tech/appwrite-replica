FROM rust:1-alpine AS build
RUN apk add --no-cache build-base
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release --locked

FROM alpine:3.22
RUN apk add --no-cache ca-certificates tzdata \
    && adduser -D -h /app openwrite \
    && mkdir -p /data \
    && chown openwrite:openwrite /data
WORKDIR /app
COPY --from=build /app/target/release/openwrite-api /usr/local/bin/openwrite-api
USER openwrite
ENV BIND_ADDR=0.0.0.0:8080 DATA_DIR=/data
EXPOSE 8080
CMD ["openwrite-api"]
