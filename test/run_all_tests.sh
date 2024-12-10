#!/bin/bash
set -e

# Variables
XRAY_BASE_URL="https://github.com/XTLS/Xray-core/releases/latest/download/Xray-linux-64.zip"
XRAY_ZIP="/tmp/xray.zip"
XRAY_DIR="/tmp/xray"
XRAY_BIN="/tmp/xray/xray"
CPROXY_BIN="./target/release/cproxy"
CPROXY_PORT=10809
TEST_URL="http://ifconfig.me"
TEST_DNS="8.8.8.8"

# Function to install Xray
install_xray() {
    echo "Installing Xray..."
    curl -L -o $XRAY_ZIP $XRAY_BASE_URL
    unzip -o $XRAY_ZIP -d $XRAY_DIR
    chmod +x $XRAY_BIN
    echo "Xray installed at $XRAY_BIN"
}

# Function to start Xray with a specific configuration
start_xray() {
    MODE=$1
    CONFIG_FILE=$2
    echo "Starting Xray with mode: $MODE"
    nohup $XRAY_BIN -config $CONFIG_FILE > /dev/null 2>&1 &
    XRAY_PID=$!
    echo "Xray started with PID $XRAY_PID"
    sleep 5 # Wait for Xray to initialize
}

# Function to stop Xray
stop_xray() {
    echo "Stopping Xray with PID $XRAY_PID..."
    kill $XRAY_PID
    wait $XRAY_PID || true
    echo "Xray stopped."
}

# Function to create Xray config for Redirect Mode
create_xray_config_redirect() {
    cat <<EOF > /tmp/xray_redirect.json
{
  "inbounds": [{
    "port": 10808,
    "protocol": "socks",
    "settings": {
      "auth": "noauth",
      "udp": true
    }
  }],
  "outbounds": [{
    "protocol": "freedom",
    "settings": {}
  }]
}
EOF
}

# Function to create Xray config for TProxy Mode
create_xray_config_tproxy() {
    cat <<EOF > /tmp/xray_tproxy.json
{
  "inbounds": [{
    "port": 10808,
    "protocol": "socks",
    "settings": {
      "auth": "noauth",
      "udp": true
    },
    "streamSettings": {
      "sockopt": {
        "tproxy": "tproxy"
      }
    }
  }],
  "outbounds": [{
    "protocol": "freedom",
    "settings": {}
  }]
}
EOF
}

# Function to create Xray config for Trace Mode
create_xray_config_trace() {
    cat <<EOF > /tmp/xray_trace.json
{
  "inbounds": [{
    "port": 10808,
    "protocol": "socks",
    "settings": {
      "auth": "noauth",
      "udp": true
    }
  }],
  "outbounds": [{
    "protocol": "freedom",
    "settings": {}
  }]
}
EOF
}

# Function to perform HTTP test via cproxy
test_http() {
    MODE=$1
    EXPECTED_IP=$2
    echo "Testing HTTP proxying for mode: $MODE"
    RESPONSE=$(curl -s --max-time 10 $TEST_URL)
    echo "Received IP: $RESPONSE"

    if [ -z "$RESPONSE" ]; then
        echo "HTTP Test Failed for mode: $MODE - No response received."
        return 1
    else
        echo "HTTP Test Passed for mode: $MODE"
    fi
}

# Function to perform DNS test via cproxy (only for suitable modes)
test_dns() {
    MODE=$1
    echo "Testing DNS proxying for mode: $MODE"
    DNS_RESPONSE=$(dig @127.0.0.1 -p $CPROXY_PORT example.com +short)
    echo "DNS Response: $DNS_RESPONSE"

    if [ -z "$DNS_RESPONSE" ]; then
        echo "DNS Test Failed for mode: $MODE - No DNS response received."
        return 1
    else
        echo "DNS Test Passed for mode: $MODE"
    fi
}

# Function to test a specific proxy mode
test_proxy_mode() {
    MODE=$1
    echo "=============================="
    echo "Testing proxy mode: $MODE"
    echo "=============================="

    # Setup Xray configuration based on mode
    case "$MODE" in
        redirect)
            create_xray_config_redirect
            ;;
        tproxy)
            create_xray_config_tproxy
            ;;
        trace)
            create_xray_config_trace
            ;;
        *)
            echo "Unknown mode: $MODE"
            exit 1
            ;;
    esac

    CONFIG_FILE="/tmp/xray_${MODE}.json"

    # Start Xray with the specific config
    start_xray "$MODE" "$CONFIG_FILE"

    # Ensure Xray is stopped after the test
    trap stop_xray EXIT

    # Start cproxy with the specific mode
    if [ "$MODE" == "trace" ]; then
        # In trace mode, we might not perform the same tests
        sudo $CPROXY_BIN --port $CPROXY_PORT --mode $MODE --pid $$ > /dev/null 2>&1 &
    else
        sudo $CPROXY_BIN --port $CPROXY_PORT --mode $MODE -- xray --port 10808 > /dev/null 2>&1 &
    fi
    CPROXY_PID=$!
    echo "cproxy started with PID $CPROXY_PID"
    sleep 5 # Wait for cproxy to set up

    # Perform HTTP test
    HTTP_RESULT=0
    if ! test_http "$MODE"; then
        HTTP_RESULT=1
    fi

    # Perform DNS test if applicable
    DNS_RESULT=0
    if [ "$MODE" != "trace" ]; then
        if ! test_dns "$MODE"; then
            DNS_RESULT=1
        fi
    fi

    # Cleanup cproxy
    echo "Stopping cproxy with PID $CPROXY_PID..."
    kill $CPROXY_PID
    wait $CPROXY_PID || true
    echo "cproxy stopped."

    # Cleanup Xray
    stop_xray

    # Reset trap
    trap - EXIT

    # Return appropriate status
    if [ "$HTTP_RESULT" -eq 1 ] || [ "$DNS_RESULT" -eq 1 ]; then
        echo "Tests failed for mode: $MODE"
        exit 1
    else
        echo "All tests passed for mode: $MODE"
    fi
}

# Main Test Execution
main() {
    install_xray

    # Test Redirect Mode
    test_proxy_mode "redirect"

    # Test TProxy Mode
    test_proxy_mode "tproxy"

    # Test Trace Mode
    test_proxy_mode "trace"

    echo "All tests completed successfully."
}

main
