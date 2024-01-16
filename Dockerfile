FROM rust:1.75-alpine AS build

RUN apk update && apk add --no-cache musl-dev

WORKDIR /usr/share/src/cf-ddns
COPY . .
RUN cargo build -q --profile release-tiny --no-default-features --features rustls-tls

FROM alpine:latest
COPY --from=build /usr/share/src/cf-ddns/target/release/cf-ddns /usr/local/bin/cf-ddns

CMD ["cf-ddns"]
