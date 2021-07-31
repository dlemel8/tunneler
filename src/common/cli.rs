use std::net::IpAddr;

use structopt::StructOpt;

#[derive(StructOpt, Debug, Copy, Clone)]
pub enum TunnelType {
    Tcp,
    Dns {
        #[structopt(env)]
        read_timeout_in_milliseconds: u64,
        #[structopt(env)]
        idle_client_timeout_in_milliseconds: u64,
    },
}

#[derive(StructOpt, Debug)]
pub struct Cli {
    #[structopt(subcommand)]
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
