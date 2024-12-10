#!/bin/bash

set -e

# Function to clean up iptables rules
cleanup_iptables() {
    echo "Cleaning up iptables rules..."
    iptables -t nat -F
    iptables -t nat -X
    iptables -t mangle -F
    iptables -t mangle -X
    iptables -t raw -F
    iptables -t raw -X
}

# Function to start a simple HTTP server
start_http_server() {
    echo "Starting simple HTTP server on port 8080..."
    python3 -m http.server 8080 > /dev/null 2>&1 &
    HTTP_SERVER_PID=$!
    echo $HTTP_SERVER_PID
}

# Function to stop the HTTP server
stop_http_server() {
    echo "Stopping HTTP server..."
    kill "$HTTP_SERVER_PID" || true
}

# Function to check if a port is listening
wait_for_port() {
    local port=$1
    echo "Waiting for port $port to be listening..."
    while ! ss -ltn | grep -q ":$port "; do
        sleep 1
    done
}

# Function to verify proxy is working
verify_proxy() {
    local proxy_port=$1
    echo "Verifying proxy on port $proxy_port..."
    if curl -s -x http://127.0.0.1:"$proxy_port" http://127.0.0.1:8080 | grep -q "Directory listing"; then
        echo "Proxy on port $proxy_port is working correctly."
    else
        echo "Proxy on port $proxy_port failed."
        exit 1
    fi
}

# Function to install required dependencies
install_dependencies() {
    echo "Installing required dependencies..."
    sudo apt-get update
    sudo apt-get install -y iptables curl python3
}
