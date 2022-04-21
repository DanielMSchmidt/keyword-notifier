#!/bin/bash

set -ex

function cleanup() {
    kill -9 "$PID_WEB"
    kill -9 "$PID_FETCHER_STACKOVERFLOW"
    kill -9 "$PID_FETCHER_TWITTER"
}

trap cleanup EXIT

cargo watch -x "run --bin web" &
PID_WEB=$!
cargo watch -x "run --bin fetcher-stackoverflow" & 
PID_FETCHER_STACKOVERFLOW=$!
cargo watch -x "run --bin fetcher-twitter" &
PID_FETCHER_TWITTER=$!

# wait for all processes to finish / forever
wait
