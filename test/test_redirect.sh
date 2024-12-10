#!/bin/bash
set -euo pipefail

LOG_DIR="test/logs"
LOG_FILE="${LOG_DIR}/redirect.log"

echo "Starting Redirect Mode Test..." | tee "$LOG_FILE"

# Start a simple HTTP proxy server using socat
PROXY_PORT=1080
sudo socat TCP-LISTEN:$PROXY_PORT,fork SOCKS4A:localhost:localhost:80 > /dev/null 2>&1 &
PROXY_PID=$!

# Function to clean up background processes
cleanup() {
    echo "Cleaning up Redirect Mode Test..." | tee -a "$LOG_FILE"
    sudo kill $PROXY_PID || true
    kill $CPROXY_PID || true
    sleep 2
}
trap cleanup EXIT

# Allow some time for the proxy to start
sleep 2

# Start cproxy in redirect mode with a simple command
sudo ./target/release/cproxy --port $PROXY_PORT --mode redirect --redirect-dns -- echo "Test Redirect" > /dev/null &
CPROXY_PID=$!

# Allow some time for cproxy to set up iptables
sleep 2

# Perform a simple HTTP request to verify redirection
echo "Performing HTTP request through redirect proxy..." | tee -a "$LOG_FILE"
curl -x socks4a://localhost:$PROXY_PORT --connect-timeout 5 --max-time 10 http://example.com -o /dev/null
if [ $? -ne 0 ]; then
    echo "HTTP request failed in Redirect mode" | tee -a "$LOG_FILE"
    exit 1
fi

# Perform a DNS request to verify DNS redirection
echo "Performing DNS request through redirect proxy..." | tee -a "$LOG_FILE"
dig @127.0.0.1 -p $PROXY_PORT example.com > "${LOG_DIR}/redirect_dns.log" 2>&1

# Verify DNS response
if grep -q "NOERROR" "${LOG_DIR}/redirect_dns.log"; then
    echo "DNS request successfully redirected in Redirect mode" | tee -a "$LOG_FILE"
else
    echo "DNS request failed in Redirect mode" | tee -a "$LOG_FILE"
    exit 1
fi

echo "Redirect Mode Test Passed" | tee -a "$LOG_FILE"
