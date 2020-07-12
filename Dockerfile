FROM alpine:latest

COPY ./target/x86_64-unknown-linux-musl/release/tunnelto_server /tunnelto_server

ENV RUST_LOG=tunnelto_server=debug
ENV RUST_BACKTRACE=1

EXPOSE 8080
EXPOSE 5000

ENTRYPOINT ["/tunnelto_server"]