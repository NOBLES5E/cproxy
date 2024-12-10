#!/bin/bash
set -euo pipefail

LOG_DIR="test/logs"
LOG_FILE="${LOG_DIR}/trace.log"

echo "Starting Trace Mode Test..." | tee "$LOG_FILE"

# Start cproxy in trace mode with a simple command
./target/release/cproxy --mode trace -- echo "Test Trace" > /dev/null &
CPROXY_PID=$!

# Function to clean up background processes
cleanup() {
    echo "Cleaning up Trace Mode Test..." | tee -a "$LOG_FILE"
    kill $CPROXY_PID || true
    sleep 2
}
trap cleanup EXIT

# Allow some time for cproxy to set up iptables
sleep 2

# Perform a simple HTTP request to generate logs
echo "Performing HTTP request to generate trace logs..." | tee -a "$LOG_FILE"
curl http://example.com -o /dev/null
if [ $? -ne 0 ]; then
    echo "HTTP request failed in Trace mode" | tee -a "$LOG_FILE"
    exit 1
fi

# Check dmesg for trace logs
echo "Checking dmesg for trace logs..." | tee -a "$LOG_FILE"
DMESG_OUTPUT=$(sudo dmesg | grep "cproxy_trace")

echo "$DMESG_OUTPUT" | tee -a "$LOG_FILE"

if [ -z "$DMESG_OUTPUT" ]; then
    echo "No trace logs found in dmesg" | tee -a "$LOG_FILE"
    exit 1
fi

echo "Trace Mode Test Passed" | tee -a "$LOG_FILE"
