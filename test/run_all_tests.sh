#!/bin/bash

set -euo pipefail

# Variables
XRAY_VERSION=""
XRAY_DOWNLOAD_URL=""
XRAY_ARCH=""
TEMP_DIR=""
XRAY_PID=""
CLOUDFLARE_DNS="1.1.1.1"

# Function to install xray
install_xray() {
    echo "Installing xray..."

    # Fetch the latest xray version from GitHub
    XRAY_VERSION=$(curl -s https://api.github.com/repos/XTLS/Xray-core/releases/latest | grep '"tag_name":' | sed -E 's/.*"v([^"]+)".*/\1/')

    if [ -z "$XRAY_VERSION" ]; then
        echo "Failed to fetch xray version."
        exit 1
    fi

    echo "Latest xray version: v$XRAY_VERSION"

    # Determine system architecture
    UNAME_ARCH=$(uname -m)
    case "$UNAME_ARCH" in
        x86_64)
            XRAY_ARCH="amd64"
            ;;
        aarch64 | armv8)
            XRAY_ARCH="arm64"
            ;;
        armv7l)
            XRAY_ARCH="armv7"
            ;;
        *)
            echo "Unsupported architecture: $UNAME_ARCH"
            exit 1
            ;;
    esac

    echo "Detected architecture: $XRAY_ARCH"

    # Construct download URL
    XRAY_DOWNLOAD_URL="https://github.com/XTLS/Xray-core/releases/download/v$XRAY_VERSION/Xray-linux-$XRAY_ARCH.zip"

    # Create temporary directory
    TEMP_DIR=$(mktemp -d)
    pushd "$TEMP_DIR" > /dev/null

    echo "Downloading xray from $XRAY_DOWNLOAD_URL"
    wget -q "$XRAY_DOWNLOAD_URL" -O xray.zip

    echo "Extracting xray..."
    unzip -q xray.zip xray

    echo "Installing xray to /usr/local/bin/"
    sudo mv xray /usr/local/bin/
    sudo chmod +x /usr/local/bin/xray

    popd > /dev/null
    rm -rf "$TEMP_DIR"

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
