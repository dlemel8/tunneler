use std::net::IpAddr;

use structopt::{clap::arg_enum, StructOpt};

arg_enum! {
    #[derive(Debug, Copy, Clone)]
    pub enum TunnelType {
        Tcp,
        Dns,
    }
}

#[derive(StructOpt, Debug)]
pub struct Cli {
    #[structopt(env, possible_values = & TunnelType::variants(), case_insensitive = true)]
    pub tunnel_type: TunnelType,

    #[structopt(env)]
    pub remote_address: IpAddr,

    #[structopt(env)]
    pub remote_port: u16,

    #[structopt(default_value = "0.0.0.0", long, env)]
    pub local_address: IpAddr,

    #[structopt(default_value = "8888", long, env)]
    pub local_port: u16,

    #[structopt(default_value = "info", long, env)]
    pub log_level: log::LevelFilter,
}
