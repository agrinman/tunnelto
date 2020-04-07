#!/bin/bash

docker run -v "cargo-cache:$HOME/.cargo" -v "$PWD:/volume" --rm -it clux/muslrust:1.44.0-nightly cargo build --bin wormhole_server --release

