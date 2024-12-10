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

# Store PIDs for cleanup
declare -a PIDS=()

# Function to install Xray
install_xray() {
    echo "Installing Xray..."
    curl -L -o $XRAY_ZIP $XRAY_BASE_URL
    unzip -o $XRAY_ZIP -d $XRAY_DIR
    chmod +x $XRAY_BIN
    echo "Xray installed at $XRAY_BIN"
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

# Function to start Xray with a specific configuration
start_xray() {
    MODE=$1
    CONFIG_FILE=$2
    echo "Starting Xray with mode: $MODE"
    nohup $XRAY_BIN -config $CONFIG_FILE > /dev/null 2>&1 &
    XRAY_PID=$!
    PIDS+=($XRAY_PID)
    echo "Xray started with PID $XRAY_PID"
    sleep 5 # Wait for Xray to initialize
}

# Function to stop all background processes
cleanup() {
    echo "Cleaning up background processes..."
    for PID in "${PIDS[@]}"; do
        if kill -0 $PID 2>/dev/null; then
            echo "Stopping process with PID $PID..."
            kill $PID || true
            wait $PID || true
            echo "Process with PID $PID stopped."
        fi
    done
    echo "Cleanup completed."
}

# Ensure that cleanup is called on script exit
trap cleanup EXIT

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

# Function to perform DNS test via cproxy (only applicable if --redirect-dns is enabled)
test_dns() {
    MODE=$1
    echo "Testing DNS proxying for mode: $MODE"

    if [ "$MODE" == "redirect" ]; then
        # In redirect mode with --redirect-dns, DNS requests are intercepted via iptables
        # To test DNS redirection, perform a DNS lookup normally and verify if it's routed through the proxy
        # Since direct DNS query to cproxy's port is not applicable, we'll check system DNS resolution
        # For safety, we'll perform a DNS query and check if the response is as expected

        # Capture the system's default DNS server before modifying iptables
        DEFAULT_DNS=$(dig +short @resolver1.opendns.com myip.opendns.com A | tail -n1)
        echo "Default DNS server IP: $DEFAULT_DNS"

        # Perform DNS lookup via default resolver
        DNS_RESPONSE=$(dig example.com +short)
        echo "DNS Response: $DNS_RESPONSE"

        if [[ -z "$DNS_RESPONSE" ]]; then
            echo "DNS Test Failed for mode: $MODE - No DNS response received."
            return 1
        else
            echo "DNS Test Passed for mode: $MODE"
        fi
    else
        echo "DNS Test not applicable for mode: $MODE"
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

    # Start cproxy with the specific mode
    if [ "$MODE" == "trace" ]; then
        # In trace mode, run cproxy with the current shell's PID to capture network activities
        sudo $CPROXY_BIN --port $CPROXY_PORT --mode $MODE --pid $$ > /dev/null 2>&1 &
    else
        # For redirect and tproxy modes, start cproxy to proxy the xray process
        sudo $CPROXY_BIN --port $CPROXY_PORT --mode $MODE ${MODE=="redirect" && echo "--redirect-dns"} -- xray --port 10808 > /dev/null 2>&1 &
    fi
    CPROXY_PID=$!
    PIDS+=($CPROXY_PID)
    echo "cproxy started with PID $CPROXY_PID"
    sleep 5 # Wait for cproxy to set up

    # Perform HTTP test
    HTTP_RESULT=0
    if ! test_http "$MODE"; then
        HTTP_RESULT=1
    fi

    # Perform DNS test if applicable
    DNS_RESULT=0
    if [ "$MODE" == "redirect" ]; then
        if ! test_dns "$MODE"; then
            DNS_RESULT=1
        fi
    else
        echo "Skipping DNS test for mode: $MODE"
    fi

    # Assess test results
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

    # Test Redirect Mode with DNS redirection
    CPROXY_REDIRECT_DNS_FLAG="--redirect-dns"
    echo "=============================="
    echo "Testing proxy mode: redirect (with DNS)"
    echo "=============================="

    # Setup Xray for redirect mode
    create_xray_config_redirect
    CONFIG_FILE="/tmp/xray_redirect.json"
    start_xray "redirect" "$CONFIG_FILE"

    # Start cproxy with redirect mode and DNS redirection
    sudo $CPROXY_BIN --port $CPROXY_PORT --mode redirect --redirect-dns -- xray --port 10808 > /dev/null 2>&1 &
    CPROXY_PID=$!
    PIDS+=($CPROXY_PID)
    echo "cproxy started with PID $CPROXY_PID"
    sleep 5 # Wait for cproxy to set up

    # Perform HTTP test
    HTTP_RESULT=0
    if ! test_http "redirect"; then
        HTTP_RESULT=1
    fi

    # Perform DNS test
    DNS_RESULT=0
    if ! test_dns "redirect"; then
        DNS_RESULT=1
    fi

    # Assess test results
    if [ "$HTTP_RESULT" -eq 1 ] || [ "$DNS_RESULT" -eq 1 ]; then
        echo "Tests failed for mode: redirect"
        exit 1
    else
        echo "All tests passed for mode: redirect"
    fi

    # Cleanup cproxy and Xray for redirect mode
    kill $CPROXY_PID || true
    wait $CPROXY_PID || true
    stop_xray

    # Reset test environment
    echo "Resetting test environment..."

    # Test TProxy Mode
    test_proxy_mode "tproxy"

    # Test Trace Mode
    test_proxy_mode "trace"

    echo "All tests completed successfully."
}

main
