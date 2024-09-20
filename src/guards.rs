use cgroups_rs::cgroup_builder::CgroupBuilder;
use cgroups_rs::{Cgroup, CgroupPid};
use eyre::Result;
use std::time::Duration;

#[allow(unused)]
pub struct CGroupGuard {
    pub pid: u32,
    pub cg: Cgroup,
    pub cg_path: String,
    pub class_id: u32,
    pub hier_v2: bool,
}

impl CGroupGuard {
    pub fn new(pid: u32) -> Result<Self> {
        let hier = cgroups_rs::hierarchies::auto();
        let hier_v2 = hier.v2();
        let class_id = pid;
        let cg_path = format!("cproxy-{}", pid);
        let cg: Cgroup = CgroupBuilder::new(cg_path.as_str())
            .network()
            .class_id(class_id as u64)
            .done()
            .build(hier);
        cg.add_task(CgroupPid::from(pid as u64)).unwrap();
        Ok(Self {
            pid,
            hier_v2,
            cg,
            cg_path,
            class_id,
        })
    }
}

impl Drop for CGroupGuard {
    fn drop(&mut self) {
        for t in self.cg.tasks() {
            self.cg.remove_task(t);
        }
        self.cg.delete().unwrap();
    }
}

#[allow(unused)]
pub struct RedirectGuard {
    port: u32,
    output_chain_name: String,
    cgroup_guard: CGroupGuard,
    redirect_dns: bool,
}

impl RedirectGuard {
    pub fn new(
        port: u32,
        output_chain_name: &str,
        cgroup_guard: CGroupGuard,
        redirect_dns: bool,
    ) -> Result<Self> {
        tracing::debug!(
            "creating redirect guard on port {}, with redirect_dns: {}",
            port,
            redirect_dns
        );
        let class_id = cgroup_guard.class_id;
        let cgroup_path = cgroup_guard.cg_path.as_str();
        (cmd_lib::run_cmd! {
        iptables -t nat -N ${output_chain_name};
        iptables -t nat -A OUTPUT -j ${output_chain_name};
        iptables -t nat -A ${output_chain_name} -p udp -o lo -j RETURN;
        iptables -t nat -A ${output_chain_name} -p tcp -o lo -j RETURN;
        })?;

        if cgroup_guard.hier_v2 {
            (cmd_lib::run_cmd! {
                iptables -t nat -A ${output_chain_name} -p tcp -m cgroup --path ${cgroup_path} -j REDIRECT --to-ports ${port};
            })?;
            if redirect_dns {
                (cmd_lib::run_cmd! {
                    iptables -t nat -A ${output_chain_name} -p udp -m cgroup --path ${cgroup_path} --dport 53 -j REDIRECT --to-ports ${port};
                })?;
            }
        } else {
            (cmd_lib::run_cmd! {
                iptables -t nat -A ${output_chain_name} -p tcp -m cgroup --cgroup ${class_id} -j REDIRECT --to-ports ${port};
            })?;
            if redirect_dns {
                (cmd_lib::run_cmd! {
                    iptables -t nat -A ${output_chain_name} -p udp -m cgroup --cgroup ${class_id} --dport 53 -j REDIRECT --to-ports ${port};
                })?;
            }
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
          iptables -t nat -D OUTPUT -j ${output_chain_name};
          iptables -t nat -F ${output_chain_name};
          iptables -t nat -X ${output_chain_name};
        })
        .expect("drop iptables and cgroup failed");
    }
}

pub struct IpRuleGuardInner {
    fwmark: u32,
    table: u32,
    guard_thread: std::thread::JoinHandle<()>,
    stop_channel: flume::Sender<()>,
}

#[allow(unused)]
pub struct IpRuleGuard {
    inner: Box<dyn Drop>,
}

impl IpRuleGuard {
    pub fn new(fwmark: u32, table: u32) -> Self {
        let (sender, receiver) = flume::unbounded();
        let thread = std::thread::spawn(move || {
            (cmd_lib::run_cmd! {
              ip rule add fwmark ${fwmark} table ${table};
              ip route add local 0.0.0.0/0 dev lo table ${table};
            })
            .expect("set routing rules failed");
            loop {
                if (cmd_lib::run_fun! { ip rule list fwmark ${fwmark} })
                    .unwrap()
                    .is_empty()
                {
                    tracing::warn!("detected disappearing routing policy, possibly due to interruped network, resetting");
                    (cmd_lib::run_cmd! {
                      ip rule add fwmark ${fwmark} table ${table};
                    })
                    .expect("set routing rules failed");
                }
                if receiver.recv_timeout(Duration::from_secs(1)).is_ok() {
                    break;
                }
            }
        });
        let inner = IpRuleGuardInner {
            fwmark,
            table,
            guard_thread: thread,
            stop_channel: sender,
        };
        let inner = with_drop::with_drop(inner, |x| {
            x.stop_channel.send(()).unwrap();
            x.guard_thread.join().unwrap();
            let mark = x.fwmark;
            let table = x.table;
            (cmd_lib::run_cmd! {
                ip rule delete fwmark ${mark} table ${table};
                ip route delete local 0.0.0.0/0 dev lo table ${table};
            })
            .expect("drop routing rules failed");
        });
        Self {
            inner: Box::new(inner),
        }
    }
}

#[allow(unused)]
pub struct TProxyGuard {
    port: u32,
    mark: u32,
    output_chain_name: String,
    prerouting_chain_name: String,
    iprule_guard: IpRuleGuard,
    cgroup_guard: CGroupGuard,
    override_dns: Option<String>,
}

impl TProxyGuard {
    pub fn new(
        port: u32,
        mark: u32,
        output_chain_name: &str,
        prerouting_chain_name: &str,
        cgroup_guard: CGroupGuard,
        override_dns: Option<String>,
    ) -> Result<Self> {
        let class_id = cgroup_guard.class_id;
        let cg_path = cgroup_guard.cg_path.as_str();
        tracing::debug!(
            "creating tproxy guard on port {}, with override_dns: {:?}",
            port,
            override_dns
        );
        let iprule_guard = IpRuleGuard::new(mark, mark);
        (cmd_lib::run_cmd! {

        iptables -t mangle -N ${prerouting_chain_name};
        iptables -t mangle -A PREROUTING -j ${prerouting_chain_name};
        iptables -t mangle -A ${prerouting_chain_name} -p tcp -o lo -j RETURN;
        iptables -t mangle -A ${prerouting_chain_name} -p udp -o lo -j RETURN;
        iptables -t mangle -A ${prerouting_chain_name} -p udp -m mark --mark ${mark} -j TPROXY --on-ip 127.0.0.1 --on-port ${port};
        iptables -t mangle -A ${prerouting_chain_name} -p tcp -m mark --mark ${mark} -j TPROXY --on-ip 127.0.0.1 --on-port ${port};

        iptables -t mangle -N ${output_chain_name};
        iptables -t mangle -A OUTPUT -j ${output_chain_name};
        iptables -t mangle -A ${output_chain_name} -p tcp -o lo -j RETURN;
        iptables -t mangle -A ${output_chain_name} -p udp -o lo -j RETURN;
        })?;

        if override_dns.is_some() {
            (cmd_lib::run_cmd! {
                iptables -t nat -N ${output_chain_name};
                iptables -t nat -A OUTPUT -j ${output_chain_name};
                iptables -t nat -A ${output_chain_name} -p udp -o lo -j RETURN;
            })?;
        }

        if cgroup_guard.hier_v2 {
            (cmd_lib::run_cmd! {
                iptables -t mangle -A ${output_chain_name} -p tcp -m cgroup --path ${cg_path} -j MARK --set-mark ${mark};
                iptables -t mangle -A ${output_chain_name} -p udp -m cgroup --path ${cg_path} -j MARK --set-mark ${mark};
            })?;
            if let Some(override_dns) = &override_dns {
                (cmd_lib::run_cmd! {
                    iptables -t nat -A ${output_chain_name} -p udp -m cgroup --path ${cg_path} --dport 53 -j DNAT --to-destination ${override_dns};
                })?;
            }
        } else {
            (cmd_lib::run_cmd! {
                iptables -t mangle -A ${output_chain_name} -p tcp -m cgroup --cgroup ${class_id} -j MARK --set-mark ${mark};
                iptables -t mangle -A ${output_chain_name} -p udp -m cgroup --cgroup ${class_id} -j MARK --set-mark ${mark};
            })?;
            if let Some(override_dns) = &override_dns {
                (cmd_lib::run_cmd! {
                    iptables -t nat -A ${output_chain_name} -p udp -m cgroup --cgroup ${class_id} --dport 53 -j DNAT --to-destination ${override_dns};
                })?;
            }
        }

        Ok(Self {
            port,
            mark,
            output_chain_name: output_chain_name.to_owned(),
            prerouting_chain_name: prerouting_chain_name.to_owned(),
            iprule_guard,
            cgroup_guard,
            override_dns,
        })
    }
}

impl Drop for TProxyGuard {
    fn drop(&mut self) {
        let output_chain_name = &self.output_chain_name;
        let prerouting_chain_name = &self.prerouting_chain_name;

        std::thread::sleep(Duration::from_millis(100));

        (cmd_lib::run_cmd! {
            iptables -t mangle -D PREROUTING -j ${prerouting_chain_name};
            iptables -t mangle -F ${prerouting_chain_name};
            iptables -t mangle -X ${prerouting_chain_name};

            iptables -t mangle -D OUTPUT -j ${output_chain_name};
            iptables -t mangle -F ${output_chain_name};
            iptables -t mangle -X ${output_chain_name};
        })
        .expect("drop iptables and cgroup failed");

        if self.override_dns.is_some() {
            (cmd_lib::run_cmd! {
            iptables -t nat -D OUTPUT -j ${output_chain_name};
            iptables -t nat -F ${output_chain_name};
            iptables -t nat -X ${output_chain_name};
            })
            .expect("drop iptables failed");
        }
    }
}

#[allow(unused)]
pub struct TraceGuard {
    prerouting_chain_name: String,
    output_chain_name: String,
    cgroup_guard: CGroupGuard,
}

impl TraceGuard {
    pub fn new(
        output_chain_name: &str,
        prerouting_chain_name: &str,
        cgroup_guard: CGroupGuard,
    ) -> Result<Self> {
        let class_id = cgroup_guard.class_id;
        (cmd_lib::run_cmd! {
        // iptables -t raw -N ${prerouting_chain_name};
        // iptables -t raw -A PREROUTING -j ${prerouting_chain_name};
        // iptables -t raw -A ${prerouting_chain_name} -p udp -j LOG;
        // iptables -t raw -A ${prerouting_chain_name} -p tcp -j LOG;

        iptables -t raw -N ${output_chain_name};
        iptables -t raw -A OUTPUT -j ${output_chain_name};
        iptables -t raw -A ${output_chain_name} -m cgroup --cgroup ${class_id} -p tcp -j LOG;
        iptables -t raw -A ${output_chain_name} -m cgroup --cgroup ${class_id} -p udp -j LOG;
        })?;

        Ok(Self {
            output_chain_name: output_chain_name.to_owned(),
            prerouting_chain_name: prerouting_chain_name.to_owned(),
            cgroup_guard,
        })
    }
}

impl Drop for TraceGuard {
    fn drop(&mut self) {
        let output_chain_name = &self.output_chain_name;
        let _prerouting_chain_name = &self.prerouting_chain_name;

        std::thread::sleep(Duration::from_millis(100));

        (cmd_lib::run_cmd! {
            // iptables -t raw -D PREROUTING -j ${prerouting_chain_name};
            // iptables -t raw -F ${prerouting_chain_name};
            // iptables -t raw -X ${prerouting_chain_name};

            iptables -t raw -D OUTPUT -j ${output_chain_name};
            iptables -t raw -F ${output_chain_name};
            iptables -t raw -X ${output_chain_name};
        })
        .expect("drop iptables and cgroup failed");
    }
}
