## Introduction

`cproxy` can redirect TCP and DNS (UDP) traffic made by a program to a local port (such as V2Ray's dokodemo-door inbound and `ss-redir`).

Compared to complicated existing transparent proxy setup, `cproxy` usage is as easy as `proxychains`, but unlike `proxychains`, it works on any program (including static linked Go programs) and redirects DNS requests.

## Installation

You can install by downloading the binary from the [release page](https://github.com/NOBLES5E/cproxy/releases) or install with `cargo`:

```
cargo install cproxy
```

## Usage

### Simple usage: just like `proxychains`

You can launch a new program with `cproxy` with:

```
cproxy --port <destination-local-port> -- <your-program> --arg1 --arg2 ...
```

### Advanced usage: proxy an existing process

With `cproxy`, you can even proxy an existing process. This is very handy when you want to proxy existing system services such as `docker`. To do this, just run

```
cproxy --port <destination-local-port> --pid <existing-process-pid>
```

## Example setup

This section provides an example setup with DNS and TCP redirection. With the following V2Ray config, you can proxy your program's DNS requests with 1.1.1.1 and the DNS server, and proxy all TCP connections.

V2Ray config:

```json
{
  "outbounds": [
    {
      "protocol": "vmess",
      "settings": {
        "vnext": [
          {
            "address": "<your-server-addr>",
            "port": <your-server-port>,
            "security": "auto",
            "users": [
              {
                "alterId": ...,
                "id": "..."
              }
            ]
          }
        ],
        "domainStrategy": "UseIP"
      },
      "streamSettings": {
        "network": "tcp"
      },
      "tag": "out"
    },
    {
      "protocol": "dns",
      "settings": {
        "network": "udp",
        "address": "1.1.1.1",
        "port": 53
      },
      "tag": "dns-out"
    }
  ],
  "dns": {
    "servers": [
      "1.1.1.1"
    ]
  },
  "routing": {
    "rules": [
      {
        "port": 1082,
        "network": "udp",
        "inboundTag": [
          "transparent"
        ],
        "outboundTag": "dns-out",
        "type": "field"
      }
    ]
  },
  "inbounds": [
    {
      "listen": "127.0.0.1",
      "port": 1082,
      "protocol": "dokodemo-door",
      "settings": {
        "followRedirect": true,
        "network": "tcp,udp"
      },
      "sniffing": {
        "destOverride": [
          "http",
          "tls"
        ],
        "enabled": true
      },
      "tag": "transparent"
    }
  ]
}
```

Then profit with:

```
cproxy --port 1082 -- <your-program> --arg1 --arg2 ...
```

## How does it work?

By utilizing linux `cgroup` `net_cls`, the implementation is very simple! Just read through https://github.com/NOBLES5E/cproxy/blob/master/src/main.rs :)

## Limitations

* `cproxy` requires `sudo` and root access to modify `cgroup`.
* Currently only tested on Linux.
* The local destination port should be a port like V2Ray `dokodemo-door`, not SOCKS5 etc.

## Similar projects

There are some awesome existing work:

* [graftcp](https://github.com/hmgle/graftcp): work on most programs, but cannot proxy UDP (such as DNS) requests.
* [proxychains](https://github.com/haad/proxychains): easy to use, but not working on static linked programs (such as Go programs).
* [proxychains-ng](https://github.com/rofl0r/proxychains-ng): similar to proxychains.
