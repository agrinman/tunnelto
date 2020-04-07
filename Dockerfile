FROM alpine:latest

COPY ./target/x86_64-unknown-linux-musl/release/wormhole_server /wormhole_server

ENV RUST_LOG=error,wormhole_server=debug
ENV RUST_BACKTRACE=1

EXPOSE 5000
EXPOSE 8080

ENTRYPOINT ["/wormhole_server"]