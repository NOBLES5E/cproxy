#!/bin/bash

set -e

source ./test/helpers.sh

cleanup_iptables

install_dependencies

# Start HTTP server
HTTP_SERVER_PID=$(start_http_server)
trap stop_http_server EXIT

# Start a simple SOCKS5 proxy using socat
echo "Starting SOCKS5 proxy on port 1080..."
socat TCP-LISTEN:1080,fork SOCKS4A:localhost:127.0.0.1:8080,socksport=1080 > /dev/null 2>&1 &
SOCKS_PROXY_PID=$!
trap "kill $SOCKS_PROXY_PID && stop_http_server" EXIT

# Verify proxy is listening
wait_for_port 1080

# Start cproxy in redirect mode
echo "Starting cproxy in redirect mode on port 1080..."
./cproxy --port 1080 --mode redirect --redirect-dns -- /bin/bash -c "sleep 30" &
CPROXY_PID=$!
sleep 5  # Give cproxy time to set up

# Verify proxy functionality
verify_proxy 1080

# Cleanup
echo "Tests passed. Cleaning up..."
kill "$CPROXY_PID" || true
