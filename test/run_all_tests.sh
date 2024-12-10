#!/bin/bash

set -euo pipefail

# Variables
TEMP_DIR=""
XRAY_PID=""
CLOUDFLARE_DNS="1.1.1.1"

# Function to install xray
install_xray() {
    echo "Installing xray..."
    bash -c "$(curl -L https://github.com/XTLS/Xray-install/raw/main/install-release.sh)" @ install
    echo "xray installation completed."
}

# Function to create xray configuration without TPROXY
create_xray_config_normal() {
    echo "Creating xray configuration for normal mode..."

    cat > /tmp/xray_config_normal.json <<EOF
{
  "inbounds": [
    {
      "port": 1082,
      "protocol": "dokodemo-door",
      "settings": {
        "network": "tcp,udp",
        "followRedirect": true
      },
      "streamSettings": {
        "network": "tcp"
      }
    }
  ],
  "outbounds": [
    {
      "protocol": "freedom",
      "settings": {}
    }
  ]
}
EOF

    echo "xray_config_normal.json created at /tmp/"
}

# Function to create xray configuration with TPROXY
create_xray_config_tproxy() {
    echo "Creating xray configuration for TPROXY mode..."

    cat > /tmp/xray_config_tproxy.json <<EOF
{
  "inbounds": [
    {
      "port": 1082,
      "listen": "0.0.0.0",
      "protocol": "dokodemo-door",
      "settings": {
        "network": "tcp,udp",
        "followRedirect": true
      },
      "streamSettings": {
        "sockopt": {
          "tproxy": "tproxy"
        }
      }
    }
  ],
  "outbounds": [
    {
      "protocol": "freedom",
      "settings": {}
    }
  ]
}
EOF

    echo "xray_config_tproxy.json created at /tmp/"
}

# Function to start xray
start_xray() {
    CONFIG_FILE=$1
    echo "Starting xray with config: $CONFIG_FILE"
    sudo xray -config "$CONFIG_FILE" &
    XRAY_PID=$!
    sleep 2
    echo "xray started with PID $XRAY_PID"
}

# Function to stop xray
stop_xray() {
    if ps -p $XRAY_PID > /dev/null 2>&1; then
        echo "Stopping xray with PID $XRAY_PID"
        sudo kill $XRAY_PID
        wait $XRAY_PID 2>/dev/null || true
        echo "xray stopped."
    fi
}

# Function to run cproxy test
run_cproxy_test() {
    MODE=$1
    echo "Running cproxy test in mode: $MODE"

    if [ "$MODE" == "tproxy" ]; then
        CPROXY_MODE="--mode tproxy"
    else
        CPROXY_MODE=""
    fi

    # Example command to test proxying using curl
    echo "Executing curl through cproxy..."
    sudo cproxy $CPROXY_MODE --port 1082 --redirect-dns -- curl -s -I https://www.google.com > /dev/null

    if [ $? -eq 0 ]; then
        echo "cproxy test in mode '$MODE': SUCCESS"
    else
        echo "cproxy test in mode '$MODE': FAILED"
        exit 1
    fi
}

# Function to clean up in case of script exit
cleanup() {
    echo "Cleaning up..."
    stop_xray
}
trap cleanup EXIT

# Main Execution Flow
main() {
    install_xray

    # Test without TPROXY
    create_xray_config_normal
    start_xray /tmp/xray_config_normal.json
    run_cproxy_test "normal"
    stop_xray

    # Test with TPROXY
    create_xray_config_tproxy
    start_xray /tmp/xray_config_tproxy.json
    run_cproxy_test "tproxy"
    stop_xray

    echo "All end-to-end tests completed successfully!"
}

main
