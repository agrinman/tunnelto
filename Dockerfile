FROM alpine:latest

COPY ./target/x86_64-unknown-linux-musl/release/tunnelto_server /tunnelto_server

# client svc
EXPOSE 8080
# ctrl svc
EXPOSE 5000
# net svc
EXPOSE 10002

ENTRYPOINT ["/tunnelto_server"]