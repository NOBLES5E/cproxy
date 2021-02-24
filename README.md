## Introduction

`cproxy` can redirect TCP and DNS (UDP) traffic made by a program to a local port.

Compared to many existing complicated transparent proxy setup, `cproxy` usage is as easy as `proxychains`, but unlike `proxychains`, it works on any program (including static linked Go programs) and redirects DNS requests.

Note: The local port for `cproxy` should be a transparent proxy port (such as V2Ray's `dokodemo-door` inbound and shadowsocks `ss-redir`). A good news is that even if you only have a SOCKS5 proxy, there are tools that can convert it to a transparent proxy for you (for example, [ipt2socks](https://github.com/zfl9/ipt2socks) and [ip2socks-go](https://github.com/lcdbin/ip2socks-go)).

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

All TCP connections and DNS requests will be proxied.

### Advanced usage: proxy an existing process

With `cproxy`, you can even proxy an existing process. This is very handy when you want to proxy existing system services such as `docker`. To do this, just run

```
cproxy --port <destination-local-port> --pid <existing-process-pid>
```

### Advanced usage: use iptables tproxy

If your system support `tproxy`, you can use `tproxy` with `--use-tproxy` flag:

```bash
cproxy --port <destination-local-port> --use-tproxy -- <your-program> --arg1 --arg2 ...
# or for existing process
cproxy --port <destination-local-port> --use-tproxy --pid <existing-process-pid>
```

With `--use-tproxy`, there are several differences:

* All TCP traffic proxied.
* All UDP traffic proxied instead of only DNS traffic to port 53.
* Your V2Ray or shadowsocks service should have `tproxy` enabled on the inbound port. For V2Ray, you need `"tproxy": "tproxy"` as in [V2Ray Documentation](https://www.v2ray.com/en/configuration/transport.html#sockoptobject). For shadowsocks, you need `-u` as shown in [shadowsocks manpage](http://manpages.org/ss-redir).

## Example setup

This section provides an example setup with DNS and TCP redirection. With the following V2Ray config, you can proxy your program's DNS requests with 1.1.1.1 as the DNS server, and proxy all TCP connections.

V2Ray config:

```
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

* [graftcp](https://github.com/hmgle/graftcp): work on most programs, but cannot proxy UDP (such as DNS) requests. `graftcp` also has performance hit on the underlying program, since it uses `ptrace`.
* [proxychains](https://github.com/haad/proxychains): easy to use, but not working on static linked programs (such as Go programs).
* [proxychains-ng](https://github.com/rofl0r/proxychains-ng): similar to proxychains.
