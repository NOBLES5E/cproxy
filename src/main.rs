use structopt::StructOpt;
use human_panic::setup_panic;

#[derive(StructOpt, Debug)]
struct Cli {
    #[structopt(long, default_value="1081")]
    port: u32,
    #[structopt(subcommand)]
    command: ChildCommand,
}

#[derive(StructOpt, Debug)]
enum ChildCommand {
    #[structopt(external_subcommand)]
    Command(Vec<String>)
}

fn main() -> anyhow::Result<()> {
    setup_panic!();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_env("LOG_LEVEL"))
        .init();
    let args: Cli = Cli::from_args();
    let pid = std::process::id();
    let ChildCommand::Command(child_command) = args.command;
    tracing::info!("subcommand {:?}", child_command);

    let cgroup_path = format!("nozomi_tproxy_{}", pid);
    let class_id = args.port;
    let port = args.port;
    (cmd_lib::run_cmd! {
        sudo mkdir -p /sys/fs/cgroup/net_cls/${cgroup_path};
        echo ${class_id} | sudo tee /sys/fs/cgroup/net_cls/${cgroup_path}/net_cls.classid > /dev/null;
        echo ${pid} | sudo tee /sys/fs/cgroup/net_cls/${cgroup_path}/cgroup.procs > /dev/null;
        sudo iptables -t nat -A OUTPUT -p tcp -m cgroup --cgroup ${port} -j REDIRECT --to-ports ${port};
        sudo iptables -t nat -A OUTPUT -p udp -m cgroup --cgroup ${port} --dport 53 -j REDIRECT --to-ports ${port};
    })?;

    let mut child = std::process::Command::new(&child_command[0]).args(&child_command[1..]).spawn()?;

    ctrlc::set_handler(move || {
        println!("received ctrl-c, terminating...");
    })?;

    child.wait()?;

    (cmd_lib::run_cmd! {
        sudo iptables -t nat -D OUTPUT -p tcp -m cgroup --cgroup ${port} -j REDIRECT --to-ports ${port};
        sudo iptables -t nat -D OUTPUT -p udp -m cgroup --cgroup ${port} --dport 53 -j REDIRECT --to-ports ${port};
        echo ${pid} | sudo tee /sys/fs/cgroup/net_cls/cgroup.procs > /dev/null;
        sudo rmdir /sys/fs/cgroup/net_cls/${cgroup_path};
    })?;

    Ok(())
}
