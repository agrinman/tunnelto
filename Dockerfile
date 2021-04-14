FROM alpine:latest

COPY ./target/x86_64-unknown-linux-musl/release/tunnelto_server /tunnelto_server

ENV RUST_LOG=tunnelto_server=debug
ENV RUST_BACKTRACE=1

# client svc
EXPOSE 8080
# ctrl svc
EXPOSE 5000
# net svc
EXPOSE 10002

ENTRYPOINT ["/tunnelto_server"]