use std::time::Duration;
use cgroups_rs::{Cgroup, CgroupPid};
use cgroups_rs::cgroup_builder::CgroupBuilder;

#[allow(unused_variables)]
pub struct CGroupGuard {
    pub pid: u32,
    pub cg: Cgroup,
    pub cg_path: String,
    pub class_id: u32,
}

impl CGroupGuard {
    pub fn new(
        pid: u32,
    ) -> anyhow::Result<Self> {
        let hier = cgroups_rs::hierarchies::auto();
        let class_id = pid;
        let cg_path = format!("cproxy-{}", pid);
        let cg: Cgroup = CgroupBuilder::new(cg_path.as_str())
            .network().class_id(class_id as u64).done()
            .build(hier);
        cg.add_task(CgroupPid::from(pid as u64)).unwrap();
        Ok(Self {
            pid,
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
    ) -> anyhow::Result<Self> {
        tracing::debug!("creating redirect guard on port {}, with redirect_dns: {}", port, redirect_dns);
        let class_id = cgroup_guard.class_id;
        let cgroup_path = cgroup_guard.cg_path.as_str();
        (cmd_lib::run_cmd! {
        iptables -t nat -N ${output_chain_name};
        iptables -t nat -A OUTPUT -j ${output_chain_name};
        iptables -t nat -A ${output_chain_name} -p udp -o lo -j RETURN;
        iptables -t nat -A ${output_chain_name} -p tcp -o lo -j RETURN;
        iptables -t nat -A ${output_chain_name} -p tcp -m cgroup --cgroup ${class_id} -j REDIRECT --to-ports ${port};
        iptables -t nat -A ${output_chain_name} -p tcp -m cgroup --path ${cgroup_path} -j REDIRECT --to-ports ${port};
        })?;

        if redirect_dns {
            (cmd_lib::run_cmd! {
            iptables -t nat -A ${output_chain_name} -p udp -m cgroup --cgroup ${class_id} --dport 53 -j REDIRECT --to-ports ${port};
            iptables -t nat -A ${output_chain_name} -p udp -m cgroup --path ${cgroup_path} --dport 53 -j REDIRECT --to-ports ${port};
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
          iptables -t nat -D OUTPUT -j ${output_chain_name};
          iptables -t nat -F ${output_chain_name};
          iptables -t nat -X ${output_chain_name};
        })
            .expect("drop iptables and cgroup failed");
    }
}

#[allow(unused)]
pub struct TProxyGuard {
    port: u32,
    mark: u32,
    output_chain_name: String,
    prerouting_chain_name: String,
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
    ) -> anyhow::Result<Self> {
        let class_id = cgroup_guard.class_id;
        let cg_path = cgroup_guard.cg_path.as_str();
        tracing::debug!("creating tproxy guard on port {}, with override_dns: {:?}", port, override_dns);
        (cmd_lib::run_cmd! {
        ip rule add fwmark ${mark} table ${mark};
        ip route add local 0.0.0.0/0 dev lo table ${mark};

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
        iptables -t mangle -A ${output_chain_name} -p tcp -m cgroup --cgroup ${class_id} -j MARK --set-mark ${mark};
        iptables -t mangle -A ${output_chain_name} -p udp -m cgroup --cgroup ${class_id} -j MARK --set-mark ${mark};
        iptables -t mangle -A ${output_chain_name} -p tcp -m cgroup --path ${cg_path} -j MARK --set-mark ${mark};
        iptables -t mangle -A ${output_chain_name} -p udp -m cgroup --path ${cg_path} -j MARK --set-mark ${mark};
        })?;

        if let Some(override_dns) = &override_dns {
            (cmd_lib::run_cmd! {
            iptables -t nat -N ${output_chain_name};
            iptables -t nat -A OUTPUT -j ${output_chain_name};
            iptables -t nat -A ${output_chain_name} -p udp -o lo -j RETURN;
            iptables -t nat -A ${output_chain_name} -p udp -m cgroup --cgroup ${class_id} --dport 53 -j DNAT --to-destination ${override_dns};
            iptables -t nat -A ${output_chain_name} -p udp -m cgroup --path ${cg_path} --dport 53 -j DNAT --to-destination ${override_dns};
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
            ip rule delete fwmark ${mark} table ${mark};
            ip route delete local 0.0.0.0/0 dev lo table ${mark};

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
            }).expect("drop iptables failed");
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
    ) -> anyhow::Result<Self> {
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
        let prerouting_chain_name = &self.prerouting_chain_name;

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
