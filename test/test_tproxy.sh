#!/bin/bash
set -euo pipefail

LOG_DIR="test/logs"
LOG_FILE="${LOG_DIR}/tproxy.log"

echo "Starting TProxy Mode Test..." | tee "$LOG_FILE"

# Check if TProxy is supported
if ! sysctl net.ipv4.tproxy 2>/dev/null | grep -q "net.ipv4.tproxy"; then
    echo "TProxy not supported on this system." | tee -a "$LOG_FILE"
    echo "Skipping TProxy Mode Test." | tee -a "$LOG_FILE"
    exit 0
fi

# Start a simple HTTP proxy server using socat
PROXY_PORT=1081
sudo socat TCP-LISTEN:$PROXY_PORT,fork SOCKS4A:localhost:localhost:80 > /dev/null 2>&1 &
PROXY_PID=$!

# Function to clean up background processes
cleanup() {
    echo "Cleaning up TProxy Mode Test..." | tee -a "$LOG_FILE"
    sudo kill $PROXY_PID || true
    kill $CPROXY_PID || true
    sleep 2
}
trap cleanup EXIT

# Allow some time for the proxy to start
sleep 2

# Start cproxy in tproxy mode with a simple command
./target/release/cproxy --port $PROXY_PORT --mode tproxy --redirect-dns -- echo "Test TProxy" > /dev/null &
CPROXY_PID=$!

# Allow some time for cproxy to set up iptables
sleep 2

# Perform a simple HTTP request to verify redirection
echo "Performing HTTP request through TProxy..." | tee -a "$LOG_FILE"
curl -x socks4a://localhost:$PROXY_PORT http://example.com -o /dev/null
if [ $? -ne 0 ]; then
    echo "HTTP request failed in TProxy mode" | tee -a "$LOG_FILE"
    exit 1
fi

# Perform a DNS request to verify DNS redirection
echo "Performing DNS request through TProxy..." | tee -a "$LOG_FILE"
dig @127.0.0.1 -p $PROXY_PORT example.com > "${LOG_DIR}/tproxy_dns.log" 2>&1

# Verify DNS response
if grep -q "NOERROR" "${LOG_DIR}/tproxy_dns.log"; then
    echo "DNS request successfully redirected in TProxy mode" | tee -a "$LOG_FILE"
else
    echo "DNS request failed in TProxy mode" | tee -a "$LOG_FILE"
    exit 1
fi

echo "TProxy Mode Test Passed" | tee -a "$LOG_FILE"
