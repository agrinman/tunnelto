<p align="center">
<img src="https://repository-images.githubusercontent.com/249120770/847ed380-a030-11ea-9421-ec9e633ec12f" align="center"/>
</p>

<p align="center">    
  <a href="https://github.com/agrinman/tunnelto/actions?query=workflow%3A%22Build+and+Release%22"><img src="https://github.com/agrinman/wormhole/workflows/Build%20and%20Release/badge.svg" alt="BuildRelease"></a>
  <a href="https://crates.io/crates/wormhole-tunnel"><img src="https://img.shields.io/crates/v/wormhole-tunnel" alt="crate"></a>
  <a href="https://hub.docker.com/r/agrinman/wormhole"><img src="https://img.shields.io/docker/v/agrinman/wormhole?label=dockerhub" alt="DockerHub"></a> 
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

Or **Download a release for your target OS here**: [tunnelto/releases](https://github.com/agrinman/tunnelto/releases)

# Usage
## Quick Start
```shell script
tunnelto --port 8000
```
The above command opens a tunnelto and starts tunneling traffic to `localhost:8000`.

## More Options:
```shell script
⇢ tunnelto --help
tunnelto 0.1.6
Expose your local web server to the internet with a public url.

USAGE:
    tunnelto [FLAGS] [OPTIONS] [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information
    -v, --verbose    A level of verbosity, and can be used multiple times

OPTIONS:
    -k, --key <key>                 Sets an API authentication key to use for this tunnel
    -p, --port <port>               Sets the port to forward incoming tunnel traffic to on localhost [default: 8000]
    -s, --subdomain <sub-domain>    Specify a sub-domain for this tunnel

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
ALLOWED_HOSTS="localhost" ALLOW_UNKNOWN_CLIENTS=1 cargo run --bin tunnelto_server

# Run a local tunnelto client talking to your local tunnelto_server
WORMHOLE_HOST="localhost" WORMHOLE_PORT=5000 TLS_OFF=1 cargo run --bin tunnelto -- start -p 8000

# Test it out!
# Remember 8080 is our local tunnelto TCP server
curl -H '<subdomain>.localhost' "http://localhost:8080/some_path?with=somequery"
```

### Server Env Vars
- `ALLOWED_HOSTS`: which hostname suffixes do we allow forwarding on
- `SECRET_KEY`: an authentication key for restricting access to your tunnelto server
- `ALLOW_UNKNOWN_CLIENTS`: a boolean flag, if set, enables unknown (no authentication) clients to use your tunnelto server. Note that unknown clients are not allowed to chose a subdomain via `-s`.


## Caveats
This implementation does not support multiple running servers (i.e. centralized coordination).
Therefore, if you deploy multiple instances of the server, it will only work if the client connects to the same instance
as the remote TCP stream.
