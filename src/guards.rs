use std::time::Duration;

#[allow(unused_variables)]
pub struct CGroupGuard {
    pub pid: u32,
    pub cgroup_path: String,
    pub class_id: u32,
    pub remove_children: bool,
}

impl CGroupGuard {
    pub fn new(
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
        (cmd_lib::run_cmd! {
        sudo iptables -t nat -N ${output_chain_name};
        sudo iptables -t nat -A OUTPUT -j ${output_chain_name};
        sudo iptables -t nat -A ${output_chain_name} -p udp -o lo -j RETURN;
        sudo iptables -t nat -A ${output_chain_name} -p tcp -o lo -j RETURN;
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
        tracing::debug!("creating tproxy guard on port {}, with override_dns: {:?}", port, override_dns);
        (cmd_lib::run_cmd! {
        sudo ip rule add fwmark ${mark} table ${mark};
        sudo ip route add local 0.0.0.0/0 dev lo table ${mark};

        sudo iptables -t mangle -N ${prerouting_chain_name};
        sudo iptables -t mangle -A PREROUTING -j ${prerouting_chain_name};
        sudo iptables -t mangle -A ${prerouting_chain_name} -p tcp -o lo -j RETURN;
        sudo iptables -t mangle -A ${prerouting_chain_name} -p udp -o lo -j RETURN;
        sudo iptables -t mangle -A ${prerouting_chain_name} -p udp -m mark --mark ${mark} -j TPROXY --on-ip 127.0.0.1 --on-port ${port};
        sudo iptables -t mangle -A ${prerouting_chain_name} -p tcp -m mark --mark ${mark} -j TPROXY --on-ip 127.0.0.1 --on-port ${port};

        sudo iptables -t mangle -N ${output_chain_name};
        sudo iptables -t mangle -A OUTPUT -j ${output_chain_name};
        sudo iptables -t mangle -A ${output_chain_name} -p tcp -o lo -j RETURN;
        sudo iptables -t mangle -A ${output_chain_name} -p udp -o lo -j RETURN;
        sudo iptables -t mangle -A ${output_chain_name} -p tcp -m cgroup --cgroup ${class_id} -j MARK --set-mark ${mark};
        sudo iptables -t mangle -A ${output_chain_name} -p udp -m cgroup --cgroup ${class_id} -j MARK --set-mark ${mark};
        })?;

        if let Some(override_dns) = &override_dns {
            (cmd_lib::run_cmd! {
            sudo iptables -t nat -N ${output_chain_name};
            sudo iptables -t nat -A OUTPUT -j ${output_chain_name};
            sudo iptables -t nat -A ${output_chain_name} -p udp -o lo -j RETURN;
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
        // sudo iptables -t raw -N ${prerouting_chain_name};
        // sudo iptables -t raw -A PREROUTING -j ${prerouting_chain_name};
        // sudo iptables -t raw -A ${prerouting_chain_name} -p udp -j LOG;
        // sudo iptables -t raw -A ${prerouting_chain_name} -p tcp -j LOG;

        sudo iptables -t raw -N ${output_chain_name};
        sudo iptables -t raw -A OUTPUT -j ${output_chain_name};
        sudo iptables -t raw -A ${output_chain_name} -m cgroup --cgroup ${class_id} -p tcp -j LOG;
        sudo iptables -t raw -A ${output_chain_name} -m cgroup --cgroup ${class_id} -p udp -j LOG;
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
            // sudo iptables -t raw -D PREROUTING -j ${prerouting_chain_name};
            // sudo iptables -t raw -F ${prerouting_chain_name};
            // sudo iptables -t raw -X ${prerouting_chain_name};

            sudo iptables -t raw -D OUTPUT -j ${output_chain_name};
            sudo iptables -t raw -F ${output_chain_name};
            sudo iptables -t raw -X ${output_chain_name};
        })
            .expect("drop iptables and cgroup failed");
    }
}
