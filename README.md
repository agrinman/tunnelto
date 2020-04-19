![Build and Release](https://github.com/agrinman/wormhole/workflows/Build%20and%20Release/badge.svg)   ![Crates.io](https://img.shields.io/crates/v/wormhole-tunnel) ![@AlexGrinman](https://img.shields.io/twitter/follow/alexgrinman?label=%40AlexGrinman)

<p align="center">
<img src="https://repository-images.githubusercontent.com/249120770/6208df00-7865-11ea-9134-cb78fe857eed" align="center" height="280px"/>
</p>

# wormhole
`wormhole` lets you expose your locally running web server via a public URL.
Written in Rust. Built completely with async-io on top of tokio.

1. [Install Wormhole](#install)
2. [Usage Instructions](#usage)
3. [How does it work?](#how-does-it-work)
4. [Self-hosting](#host-it-yourself)

# Install
## Brew (macOS)
```bash
brew install agrinman/tap/wormhole
```

## Cargo
```bash
cargo install wormhole-tunnel
```

Or **Download a release for your target OS here**: [wormhole/releases](https://github.com/agrinman/wormhole/releases)

# Usage
## Quick Start
```shell script
⇢ wormhole start -p 8000
```
The above command opens a wormhole and starts tunneling traffic to `localhost:8000`.

## More Options:
```shell script
⇢ wormhole start -h
wormhole-start 0.1.4
Start the wormhole

USAGE:
    wormhole start [OPTIONS] --port <port>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -k, --key <key>                 Sets an API authentication key to use for this wormhole
    -p, --port <port>               Sets the port to forward incoming tunnel traffic to on localhost
    -s, --subdomain <sub-domain>    Specify a sub-domain for this wormhole
```

# How does it work?
## Server
The wormhole server both the operates control server (via port 5000, using websockets) and accepts 
raw TCP streams (via port 8080).

1. New clients connect over websockets to establish the `client tunnel`. 
2. When a new raw TCP stream connects, the wormhole server reads the incoming bytes and writes it to the `client tunnel`
3. When incoming data is received from the `client tunnel`, the control server writes those bytes into the TCP stream.

## Client
The wormhole client establishes a websocket connections to the wormhole server to establish the `remote tunnel`,
and then:

1. Reads TCP bytes from the `remote tunnel`
2. Opens a new local TCP stream to the locally running server (specified as a program argument)
3. Writes the TCP bytes from to the local stream
4. Reads TCP bytes from the local stream
5. Writes the outgoing TCP bytes bacl into the `remote tunnel`

## Caveats
This implementation does not support multiple running servers (i.e. centralized coordination).
Therefore, if you deploy multiple instances of the server, it will only work if the client connects to the same instance
as the remote TCP stream.

# Host it yourself
1. Compile the server for the musl target. See the `musl_build.sh` for a way to do this trivially with Docker!
2. See `Dockerfile` for a simple alpine based image that runs that server binary.
3. Deploy the image where ever you want.

## Testing Locally
```shell script
# Run the Server: xpects TCP traffic on 8080 and control websockets on 5000
ALLOWED_HOSTS="localhost" ALLOW_UNKNOWN_CLIENTS=1 cargo run --bin wormhole_server

# Run a local wormhole talking to your local wormhole_server
WORMHOLE_HOST="localhost" WORMHOLE_PORT=5000 TLS_OFF=1 cargo run --bin wormhole -- start -p 8000

# Test it out!
# Remember 8080 is our local wormhole_server TCP server
curl -H '<subdomain>.localhost' "http://localhost:8080/some_path?with=somequery"
```

### Server Env Vars
- `ALLOWED_HOSTS`: which hostname suffixes do we allow forwarding on
- `SECRET_KEY`: an authentication key for restricting access to your wormhole server
- `ALLOW_UNKNOWN_CLIENTS`: a boolean flag, if set, enables unknown (no authentication) clients to use your wormhole. Note that unknown clients are not allowed to chose a subdomain via `-s`.