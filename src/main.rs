use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
struct Cli {
    /// Redirect traffic to specific local port.
    #[structopt(long, env = "CPROXY_PORT")]
    port: u32,
    /// Do not redirect DNS traffic. This option only works without tproxy.
    #[structopt(long)]
    no_dns: bool,
    /// Enable tproxy mode.
    #[structopt(long)]
    use_tproxy: bool,
    /// Override dns server address. This option only works with tproxy mode
    #[structopt(long)]
    override_dns: Option<String>,
    /// Proxy an existing process.
    #[structopt(long)]
    pid: Option<u32>,
    #[structopt(subcommand)]
    command: Option<ChildCommand>,
}

#[derive(StructOpt, Debug)]
enum ChildCommand {
    #[structopt(external_subcommand)]
    Command(Vec<String>),
}

struct CGroupGuard {
    pub pid: u32,
    pub cgroup_path: String,
    pub class_id: u32,
    pub remove_children: bool,
}

impl CGroupGuard {
    fn new(
        pid: u32,
        cgroup_path: &str,
        remove_children: bool,
        class_id: u32,
    ) -> anyhow::Result<Self> {
        (cmd_lib::run_cmd! {
        sudo mkdir -p /sys/fs/cgroup/net_cls/${cgroup_path};
        echo ${class_id} | sudo tee /sys/fs/cgroup/net_cls/${cgroup_path}/net_cls.classid > /dev/null;
        echo ${pid} | sudo tee /sys/fs/cgroup/net_cls/${cgroup_path}/cgroup.procs > /dev/null;
        })?;

        Ok(Self {
            pid,
            cgroup_path: cgroup_path.to_owned(),
            class_id,
            remove_children,
        })
    }
}

impl Drop for CGroupGuard {
    fn drop(&mut self) {
        let cgroup_path = &self.cgroup_path;
        let pid = self.pid;
        match self.remove_children {
            true => {
                (cmd_lib::run_cmd! {
                 cat /sys/fs/cgroup/net_cls/${cgroup_path}/cgroup.procs | xargs -I "{}" bash -c "echo {} | sudo tee /sys/fs/cgroup/net_cls/cgroup.procs > /dev/null";
                 sudo rmdir /sys/fs/cgroup/net_cls/${cgroup_path};
                 }).expect("drop cgroup failed");
            }
            false => {
                (cmd_lib::run_cmd! {
                echo ${pid} | sudo tee /sys/fs/cgroup/net_cls/cgroup.procs > /dev/null;
                sudo rmdir /sys/fs/cgroup/net_cls/${cgroup_path};
                })
                    .expect("drop cgroup failed");
            }
        }
    }
}

struct RedirectGuard {
    port: u32,
    output_chain_name: String,
    cgroup_guard: CGroupGuard,
    redirect_dns: bool,
}

impl RedirectGuard {
    fn new(
        port: u32,
        output_chain_name: &str,
        cgroup_guard: CGroupGuard,
        redirect_dns: bool,
    ) -> anyhow::Result<Self> {
        tracing::debug!("creating redirect guard on port {}, with redirect_dns: {}", port, redirect_dns);
        let class_id = cgroup_guard.class_id;
        (cmd_lib::run_cmd! {
        sudo iptables -t nat -N ${output_chain_name};
        sudo iptables -t nat -A OUTPUT -j ${output_chain_name};
        sudo iptables -t nat -A ${output_chain_name} -p tcp -m cgroup --cgroup ${class_id} -j REDIRECT --to-ports ${port};
        })?;

        if redirect_dns {
            (cmd_lib::run_cmd! {
            sudo iptables -t nat -A ${output_chain_name} -p udp -m cgroup --cgroup ${class_id} --dport 53 -j REDIRECT --to-ports ${port};
            })?;
        }

        Ok(Self {
            port,
            output_chain_name: output_chain_name.to_owned(),
            cgroup_guard,
            redirect_dns,
        })
    }
}

impl Drop for RedirectGuard {
    fn drop(&mut self) {
        let output_chain_name = &self.output_chain_name;

        (cmd_lib::run_cmd! {
          sudo iptables -t nat -D OUTPUT -j ${output_chain_name};
          sudo iptables -t nat -F ${output_chain_name};
          sudo iptables -t nat -X ${output_chain_name};
        })
            .expect("drop iptables and cgroup failed");
    }
}

fn proxy_new_command(args: &Cli) -> anyhow::Result<()> {
    let pid = std::process::id();
    let ChildCommand::Command(child_command) = &args
        .command
        .as_ref()
        .expect("must have command specified if --pid not provided");
    tracing::info!("subcommand {:?}", child_command);

    let cgroup_path = format!("nozomi_tproxy_{}", pid);
    let class_id = args.port;
    let port = args.port;
    let output_chain_name = format!("nozomi_tproxy_out_{}", pid);

    let cgroup_guard = CGroupGuard::new(pid, cgroup_path.as_str(), false, class_id)?;
    let _guard = RedirectGuard::new(port, output_chain_name.as_str(), cgroup_guard, !args.no_dns)?;

    let mut child = std::process::Command::new(&child_command[0])
        .env("CPROXY_ENV", format!("cproxy/{}", port))
        .args(&child_command[1..])
        .spawn()?;

    ctrlc::set_handler(move || {
        println!("received ctrl-c, terminating...");
    })?;

    child.wait()?;

    Ok(())
}

fn proxy_existing_pid(pid: u32, args: &Cli) -> anyhow::Result<()> {
    let cgroup_path = format!("nozomi_tproxy_{}", pid);
    let class_id = args.port;
    let port = args.port;
    let output_chain_name = format!("nozomi_tproxy_out_{}", pid);

    let cgroup_guard = CGroupGuard::new(pid, cgroup_path.as_str(), true, class_id)?;
    let _guard = RedirectGuard::new(port, output_chain_name.as_str(), cgroup_guard, !args.no_dns)?;

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        println!("received ctrl-c, terminating...");
        r.store(false, Ordering::SeqCst);
    })?;

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
    }

    Ok(())
}

struct TProxyGuard {
    port: u32,
    mark: u32,
    output_chain_name: String,
    prerouting_chain_name: String,
    cgroup_guard: CGroupGuard,
    override_dns: Option<String>,
}

impl TProxyGuard {
    fn new(
        port: u32,
        mark: u32,
        output_chain_name: &str,
        prerouting_chain_name: &str,
        cgroup_guard: CGroupGuard,
        override_dns: Option<String>,
    ) -> anyhow::Result<Self> {
        let class_id = cgroup_guard.class_id;
        tracing::debug!("creating tproxy guard on port {}, with override_dns: {:?}", port, override_dns);
        (cmd_lib::run_cmd! {
        sudo ip rule add fwmark ${mark} table ${mark};
        sudo ip route add local 0.0.0.0/0 dev lo table ${mark};

        sudo iptables -t mangle -N ${prerouting_chain_name};
        sudo iptables -t mangle -A PREROUTING -j ${prerouting_chain_name};
        sudo iptables -t mangle -A ${prerouting_chain_name} -p udp -m mark --mark ${mark} -j TPROXY --on-ip 127.0.0.1 --on-port ${port};
        sudo iptables -t mangle -A ${prerouting_chain_name} -p tcp -m mark --mark ${mark} -j TPROXY --on-ip 127.0.0.1 --on-port ${port};

        sudo iptables -t mangle -N ${output_chain_name};
        sudo iptables -t mangle -A OUTPUT -j ${output_chain_name};
        sudo iptables -t mangle -A ${output_chain_name} -p tcp -m cgroup --cgroup ${class_id} -j MARK --set-mark ${mark};
        sudo iptables -t mangle -A ${output_chain_name} -p udp -m cgroup --cgroup ${class_id} -j MARK --set-mark ${mark};
        })?;

        if let Some(override_dns) = &override_dns {
            (cmd_lib::run_cmd! {
            sudo iptables -t nat -N ${output_chain_name};
            sudo iptables -t nat -A OUTPUT -j ${output_chain_name};
            sudo iptables -t nat -A ${output_chain_name} -p udp -m cgroup --cgroup ${class_id} --dport 53 -j DNAT --to-destination ${override_dns};
            })?;
        }

        Ok(Self {
            port,
            mark,
            output_chain_name: output_chain_name.to_owned(),
            prerouting_chain_name: prerouting_chain_name.to_owned(),
            cgroup_guard,
            override_dns,
        })
    }
}

impl Drop for TProxyGuard {
    fn drop(&mut self) {
        let output_chain_name = &self.output_chain_name;
        let prerouting_chain_name = &self.prerouting_chain_name;
        let mark = self.mark;

        std::thread::sleep(Duration::from_millis(100));

        (cmd_lib::run_cmd! {
            sudo ip rule delete fwmark ${mark} table ${mark};
            sudo ip route delete local 0.0.0.0/0 dev lo table ${mark};

            sudo iptables -t mangle -D PREROUTING -j ${prerouting_chain_name};
            sudo iptables -t mangle -F ${prerouting_chain_name};
            sudo iptables -t mangle -X ${prerouting_chain_name};

            sudo iptables -t mangle -D OUTPUT -j ${output_chain_name};
            sudo iptables -t mangle -F ${output_chain_name};
            sudo iptables -t mangle -X ${output_chain_name};
        })
            .expect("drop iptables and cgroup failed");

        if self.override_dns.is_some() {
            (cmd_lib::run_cmd! {
            sudo iptables -t nat -D OUTPUT -j ${output_chain_name};
            sudo iptables -t nat -F ${output_chain_name};
            sudo iptables -t nat -X ${output_chain_name};
            }).expect("drop iptables failed");
        }
    }
}

fn proxy_new_command_tproxy(args: &Cli) -> anyhow::Result<()> {
    let pid = std::process::id();
    let ChildCommand::Command(child_command) = &args
        .command
        .as_ref()
        .expect("must have command specified if --pid not provided");
    tracing::info!("subcommand {:?}", child_command);

    let cgroup_path = format!("nozomi_tproxy_{}", pid);
    let prerouting_chain_name = format!("nozomi_tproxy_pre_{}", pid);
    let output_chain_name = format!("nozomi_tproxy_out_{}", pid);
    let class_id = args.port;
    let port = args.port;
    let mark = pid;

    let cgroup_guard = CGroupGuard::new(pid, cgroup_path.as_str(), false, class_id)?;
    let _guard = TProxyGuard::new(
        port,
        mark,
        output_chain_name.as_str(),
        prerouting_chain_name.as_str(),
        cgroup_guard,
        args.override_dns.clone(),
    )?;

    let mut child = std::process::Command::new(&child_command[0])
        .env("CPROXY_ENV", format!("cproxy/{}", port))
        .args(&child_command[1..])
        .spawn()?;
    ctrlc::set_handler(move || {
        println!("received ctrl-c, terminating...");
    })?;
    child.wait()?;
    Ok(())
}

fn proxy_existing_pid_tproxy(pid: u32, args: &Cli) -> anyhow::Result<()> {
    let cgroup_path = format!("nozomi_tproxy_{}", pid);
    let prerouting_chain_name = format!("nozomi_tproxy_pre_{}", pid);
    let output_chain_name = format!("nozomi_tproxy_out_{}", pid);
    let class_id = args.port;
    let port = args.port;
    let mark = pid;

    let cgroup_guard = CGroupGuard::new(pid, cgroup_path.as_str(), true, class_id)?;
    let _guard = TProxyGuard::new(
        port,
        mark,
        output_chain_name.as_str(),
        prerouting_chain_name.as_str(),
        cgroup_guard,
        args.override_dns.clone(),
    )?;

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        println!("received ctrl-c, terminating...");
        r.store(false, Ordering::SeqCst);
    })?;

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_env("LOG_LEVEL"))
        .init();
    let args: Cli = Cli::from_args();

    match args.pid {
        None => match args.use_tproxy {
            true => {
                proxy_new_command_tproxy(&args)?;
            }
            false => {
                proxy_new_command(&args)?;
            }
        },
        Some(existing_pid) => match args.use_tproxy {
            true => {
                proxy_existing_pid_tproxy(existing_pid, &args)?;
            }
            false => {
                proxy_existing_pid(existing_pid, &args)?;
            }
        },
    }

    Ok(())
}
