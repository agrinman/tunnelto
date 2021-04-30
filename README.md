<p align="center" >
<img width="540px" src="https://repository-images.githubusercontent.com/249120770/7ea6d180-b4ba-11ea-96ab-6c3b987aac9d" align="center"/>
</p>

<p align="center">    
  <a href="https://github.com/agrinman/tunnelto/actions?query=workflow%3A%22Build+and+Release%22"><img src="https://github.com/agrinman/wormhole/workflows/Build%20and%20Release/badge.svg" alt="BuildRelease"></a>
  <a href="https://crates.io/crates/wormhole-tunnel"><img src="https://img.shields.io/crates/v/tunnelto" alt="crate"></a>
  <a href="https://github.com/agrinman/tunnelto/packages/295195"><img src="https://img.shields.io/docker/v/agrinman/wormhole?label=Docker" alt="GitHub Docker Registry"></a> 
  <a href="https://twitter.com/alexgrinman"><img src="https://img.shields.io/twitter/follow/alexgrinman?label=%40AlexGrinman" alt="crate"></a>
</p>

# `tunnelto`
`tunnelto` lets you expose your locally running web server via a public URL.
Written in Rust. Built completely with async-io on top of tokio.

1. [Install](#install)
2. [Usage Instructions](#usage)
3. [Host it yourself](#host-it-yourself)

# Install
## Brew (macOS)
```bash
brew install agrinman/tap/tunnelto
```

## Cargo
```bash
cargo install tunnelto
```

## Everywhere
Or **Download a release for your target OS here**: [tunnelto/releases](https://github.com/agrinman/tunnelto/releases)

# Usage
## Quick Start
```shell script
tunnelto --port 8000
```
The above command opens a tunnel and forwards traffic to `localhost:8000`.

## More Options:
```shell script
tunnelto 0.1.14

USAGE:
    tunnelto [FLAGS] [OPTIONS] [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information
    -v, --verbose    A level of verbosity, and can be used multiple times

OPTIONS:
        --dashboard-address <dashboard-address>    Sets the address of the local introspection dashboard
    -k, --key <key>                                Sets an API authentication key to use for this tunnel
        --host <local-host>
            Sets the HOST (i.e. localhost) to forward incoming tunnel traffic to [default: localhost]

    -p, --port <port>
            Sets the port to forward incoming tunnel traffic to on the target host

        --scheme <scheme>
            Sets the SCHEME (i.e. http or https) to forward incoming tunnel traffic to [default: http]

    -s, --subdomain <sub-domain>                   Specify a sub-domain for this tunnel

SUBCOMMANDS:
    help        Prints this message or the help of the given subcommand(s)
    set-auth    Store the API Authentication key
```

# Host it yourself
1. Compile the server for the musl target. See the `musl_build.sh` for a way to do this trivially with Docker!
2. See `Dockerfile` for a simple alpine based image that runs that server binary.
3. Deploy the image where ever you want.

## Testing Locally
```shell script
# Run the Server: xpects TCP traffic on 8080 and control websockets on 5000
ALLOWED_HOSTS="localhost" cargo run --bin tunnelto_server

# Run a local tunnelto client talking to your local tunnelto_server
CTRL_HOST="localhost" CTRL_PORT=5000 CTRL_TLS_OFF=1 cargo run --bin tunnelto -- -p 8000

# Test it out!
# Remember 8080 is our local tunnelto TCP server
curl -H '<subdomain>.localhost' "http://localhost:8080/some_path?with=somequery"
```
See `tunnelto_server/src/config.rs` for the environment variables for configuration.

## Caveats for hosting it yourself
The implementation does not support multiple running servers (i.e. centralized coordination).
Therefore, if you deploy multiple instances of the server, it will only work if the client connects to the same instance
as the remote TCP stream.

The [version hosted by us](https://tunnelto.dev) is a proper distributed system running on the the fabulous [fly.io](https://fly.io) service. 
In short, fly.io makes this super easy with their [Private Networking](https://fly.io/docs/reference/privatenetwork/) feature.
See `tunnelto_server/src/network/mod.rs` for the implementation details of our gossip mechanism.
