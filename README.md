<p align="center">
<img width="250px" src="https://user-images.githubusercontent.com/18649508/139117888-4f631b07-0b40-4d24-b478-fb805ceef689.png" />
</p>
<hr/>

[![Crates.io](https://img.shields.io/crates/v/cproxy)](https://crates.io/crates/cproxy) [![CI](https://github.com/NOBLES5E/cproxy/actions/workflows/build.yml/badge.svg)](https://github.com/NOBLES5E/cproxy/actions/workflows/build.yml) ![Crates.io](https://img.shields.io/crates/d/cproxy) ![Crates.io](https://img.shields.io/crates/l/cproxy)

Ever wished you could make your stubborn programs use a proxy without them even knowing? Well, say hello to `cproxy`.

## Key Features

- Transparent redirection of TCP and UDP traffic
- Support for different proxies per application/process
- Compatible with all programs, including statically linked Go binaries
- DNS request redirection
- Simple usage similar to `proxychains`
- Ability to proxy existing running processes
- Support for both iptables `REDIRECT` and `TPROXY` modes
- DNS server override in `TPROXY` mode
- Network activity tracing using iptables `LOG` target
- Compatible with cgroup v1 and v2
- No background daemon required
- Easy integration with existing software like V2Ray, Xray, and Shadowsocks

> [!TIP]
> Your proxy should be a transparent proxy port (like V2Ray's `dokodemo-door` inbound or shadowsocks `ss-redir`). But don't panic if you only have a SOCKS5 or HTTP proxy! There are tools that can transform it [faster than Bill Clinton](https://youtu.be/Dv0PxINy2ds?t=570) (check out [transocks](https://github.com/cybozu-go/transocks), [ipt2socks](https://github.com/zfl9/ipt2socks) and [ip2socks-go](https://github.com/lcdbin/ip2socks-go)).

## Installation

You can install by downloading the binary from the [release page](https://github.com/NOBLES5E/cproxy/releases) or install with: `cargo install cproxy`.

Alternatively, here's a oneliner that downloads the latest release and put it in your `/usr/local/bin/` (for the lazy... I mean, efficient folks):

```
curl -s https://api.github.com/repos/NOBLES5E/cproxy/releases/latest | grep "browser_download_url.*x86_64-unknown-linux-musl.zip" | cut -d : -f 2,3 | tr -d \" | wget -qi - -O /tmp/cproxy.zip && unzip -j /tmp/cproxy.zip cproxy -d /tmp && sudo mv /tmp/cproxy /usr/local/bin/ && sudo chmod +x /usr/local/bin/cproxy && rm /tmp/cproxy.zip
```

## Usage

### Basic Magic Trick: Just Like `proxychains`

You can launch a new program with `cproxy` with:

```
sudo cproxy --port <destination-local-port> -- <your-program> --arg1 --arg2 ...
```

All TCP connections requests will be proxied. If your local transparent proxy support DNS address overriding, you can
also redirect DNS traffic with `--redirect-dns`:

```
sudo cproxy --port <destination-local-port> --redirect-dns -- <your-program> --arg1 --arg2 ...
```

For an example setup, see [wiki](https://github.com/NOBLES5E/cproxy/wiki/Example-setup-with-V2Ray).

> [!NOTE]
> Scared of `sudo` in the command? Well, that's what we need to have the permission to modify cgroup. But don't worry too much, the program you run will still be run under your original user, not as root. `cproxy` automatically drops privileges after setting up the necessary cgroup configurations, ensuring that your program runs with the same permissions as if you had launched it directly.

### The TPROXY Twist

If your system support `tproxy`, you can use `tproxy` with `--mode tproxy`:

```bash
sudo cproxy --port <destination-local-port> --mode tproxy -- <your-program> --arg1 --arg2 ...
# or for existing process
sudo cproxy --port <destination-local-port> --mode tproxy --pid <existing-process-pid>
```

With `--mode tproxy`, there are several differences:

* All UDP traffic are proxied instead of only DNS UDP traffic to port 53.
* Your V2Ray or shadowsocks service should have `tproxy` enabled on the inbound port. For V2Ray, you
  need `"tproxy": "tproxy"` as
  in [V2Ray Documentation](https://www.v2ray.com/en/configuration/transport.html#sockoptobject). For shadowsocks, you
  need `-u` as shown in [shadowsocks manpage](http://manpages.org/ss-redir).

An example setup can be found [here](https://github.com/NOBLES5E/cproxy/wiki/Example-setup-with-V2Ray).

Note that when you are using the `tproxy` mode, you can override the DNS server address
with `cproxy --mode tproxy --override-dns <your-dns-server-addr> ...`. This is useful when you want to use a different
DNS server for a specific application.

### Advanced Usage: Proxy an Existing Process

With `cproxy`, you can even proxy an existing process. This is very handy when you want to proxy existing system
services such as `docker`. To do this, just run

```
sudo cproxy --port <destination-local-port> --pid <existing-process-pid>
```

The target process will be proxied as long as this `cproxy` command is running. You can press Ctrl-C to stop proxying.

### Advanced Usage: Debug a Program's Network Activity with Iptables LOG Target

With `cproxy`, you can easily debug a program's traffic in netfilter. Just run the program with

```bash
sudo cproxy --mode trace <your-program>
```

You will be able to see log in `dmesg`. Note that this requires a recent enough kernel and iptables.

## The Secret Sauce

`cproxy` simply creates a unique `cgroup` for the proxied program, and redirect its traffic with packet rules.

## Limitations

* `cproxy` requires root access to modify `cgroup`.
* Currently only tested on Linux.

## Similar Projects

There are some awesome existing work:

* [graftcp](https://github.com/hmgle/graftcp): work on most programs, but cannot proxy UDP (such as DNS)
  requests. `graftcp` also has performance hit on the underlying program, since it uses `ptrace`.
* [proxychains](https://github.com/haad/proxychains): easy to use, but not working on static linked programs (such as Go
  programs).
* [proxychains-ng](https://github.com/rofl0r/proxychains-ng): similar to proxychains.
* [cgproxy](https://github.com/springzfx/cgproxy): `cgproxy` also uses cgroup to do transparent proxy, and the idea is
  similar to `cproxy`'s. There are some differences in UX and system requirements:
    * `cgproxy` requires system `cgroup` v2 support, while `cproxy` works with both v1 and v2.
    * `cgproxy` requires a background daemon process `cgproxyd` running, while `cproxy` does not.
    * `cgproxy` requires `tproxy`, which is optional in `cproxy`.
    * `cgproxy` can be used to do global proxy, while `cproxy` does not intended to support global proxy.
