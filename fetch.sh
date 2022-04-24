#!/bin/bash

set -ex

echo "Fetching data..."

LOG_FILE=/home/fetch.log

/usr/local/bin/fetcher-stackoverflow > $LOG_FILE 2>&1
/usr/local/bin/fetcher-twitter > $LOG_FILE 2>&1

echo "Done fetching data"
