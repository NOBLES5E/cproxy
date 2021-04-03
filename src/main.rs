use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use structopt::StructOpt;

use guards::{CGroupGuard, RedirectGuard, TProxyGuard};
use crate::guards::TraceGuard;

mod guards;

#[derive(StructOpt, Debug)]
struct Cli {
    /// Redirect traffic to specific local port.
    #[structopt(long, env = "CPROXY_PORT", default_value = "1080")]
    port: u32,
    /// redirect DNS traffic. This option only works with redirect mode
    #[structopt(long)]
    redirect_dns: bool,
    /// Proxy mode can be `trace` (use iptables TRACE target to debug program network), `tproxy`, or `redirect`.
    #[structopt(long, default_value = "redirect")]
    mode: String,
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

fn proxy_new_command(args: &Cli) -> anyhow::Result<()> {
    let pid = std::process::id();
    let ChildCommand::Command(child_command) = &args
        .command
        .as_ref()
        .expect("must have command specified if --pid not provided");
    tracing::info!("subcommand {:?}", child_command);

    let port = args.port;
    let output_chain_name = format!("nozomi_tproxy_out_{}", pid);

    let cgroup_guard = CGroupGuard::new(pid)?;
    let _guard: Box<dyn Drop> = match args.mode.as_str() {
        "redirect" => { Box::new(RedirectGuard::new(port, output_chain_name.as_str(), cgroup_guard, args.redirect_dns)?) }
        "tproxy" => {
            let prerouting_chain_name = format!("nozomi_tproxy_pre_{}", pid);
            let mark = pid;
            Box::new(
                TProxyGuard::new(
                    port,
                    mark,
                    output_chain_name.as_str(),
                    prerouting_chain_name.as_str(),
                    cgroup_guard,
                    args.override_dns.clone(),
                )?
            )
        }
        "trace" => {
            let prerouting_chain_name = format!("nozomi_tproxy_pre_{}", pid);
            Box::new(TraceGuard::new(
                output_chain_name.as_str(),
                prerouting_chain_name.as_str(),
                cgroup_guard,
            )?)
        }
        &_ => { unimplemented!() }
    };

    let original_uid = nix::unistd::getuid();
    nix::unistd::seteuid(original_uid)?;
    let mut child = std::process::Command::new(&child_command[0])
        .env("CPROXY_ENV", format!("cproxy/{}", port))
        .args(&child_command[1..])
        .spawn()?;
    nix::unistd::seteuid(nix::unistd::Uid::from_raw(0))?;

    ctrlc::set_handler(move || {
        println!("received ctrl-c, terminating...");
    })?;

    child.wait()?;

    Ok(())
}

fn proxy_existing_pid(pid: u32, args: &Cli) -> anyhow::Result<()> {
    let port = args.port;
    let output_chain_name = format!("nozomi_tproxy_out_{}", pid);

    let cgroup_guard = CGroupGuard::new(pid)?;
    let _guard: Box<dyn Drop> = match args.mode.as_str() {
        "redirect" => { Box::new(RedirectGuard::new(port, output_chain_name.as_str(), cgroup_guard, !args.redirect_dns)?) }
        "tproxy" => {
            let prerouting_chain_name = format!("nozomi_tproxy_pre_{}", pid);
            let mark = pid;
            Box::new(
                TProxyGuard::new(
                    port,
                    mark,
                    output_chain_name.as_str(),
                    prerouting_chain_name.as_str(),
                    cgroup_guard,
                    args.override_dns.clone(),
                )?
            )
        }
        "trace" => {
            let prerouting_chain_name = format!("nozomi_tproxy_pre_{}", pid);
            Box::new(TraceGuard::new(
                output_chain_name.as_str(),
                prerouting_chain_name.as_str(),
                cgroup_guard,
            )?)
        }
        _ => { unimplemented!() }
    };

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
    nix::unistd::seteuid(nix::unistd::Uid::from_raw(0)).expect(
        "cproxy failed to seteuid, please `chown root:root` and `chmod +s` on cproxy binary"
    );
    let args: Cli = Cli::from_args();

    match args.pid {
        None => {
            proxy_new_command(&args)?;
        }
        Some(existing_pid) => {
            proxy_existing_pid(existing_pid, &args)?;
        }
    }

    Ok(())
}
