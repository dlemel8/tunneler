use std::error::Error;
use std::net::IpAddr;
use std::path::PathBuf;
use std::str::FromStr;

use structopt::{clap::arg_enum, StructOpt};
use trust_dns_resolver::Resolver;

#[derive(StructOpt, Debug)]
pub enum TunnelerType {
    Tcp,
    Dns {
        #[structopt(env)]
        read_timeout_in_milliseconds: u64,
        #[structopt(env)]
        idle_client_timeout_in_milliseconds: u64,
        #[structopt(default_value = "", env)]
        client_suffix: String,
    },
    Tls {
        #[structopt(env)]
        ca_cert: PathBuf,
        #[structopt(env)]
        cert: PathBuf,
        #[structopt(env)]
        key: PathBuf,
    },
}

arg_enum! {
    #[derive(Debug)]
    pub enum TunneledType {
        Tcp,
        Udp,
    }
}

fn parse_or_resolve_ip(src: &str) -> Result<IpAddr, Box<dyn Error>> {
    IpAddr::from_str(src).or_else(|_| {
        let resolver = Resolver::from_system_conf()?;
        let response = resolver.lookup_ip(src)?;
        response
            .iter()
            .next()
            .ok_or_else(|| format!("failed to parse or resolve {} to an IP", src).into())
    })
}

#[derive(StructOpt, Debug)]
pub struct Cli {
    #[structopt(subcommand)]
    pub tunneler_type: TunnelerType,

    #[structopt(env, possible_values = &TunneledType::variants(), case_insensitive = true)]
    pub tunneled_type: TunneledType,

    #[structopt(env, parse(try_from_str = parse_or_resolve_ip))]
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

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn parse_or_resolve_ip_ipv4() -> Result<(), Box<dyn Error>> {
        let res = parse_or_resolve_ip("127.0.0.1")?;
        assert_eq!(res, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        Ok(())
    }

    #[test]
    fn parse_or_resolve_ip_ipv6() -> Result<(), Box<dyn Error>> {
        let res = parse_or_resolve_ip("::1")?;
        assert_eq!(res, IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)));
        Ok(())
    }

    #[test]
    fn parse_or_resolve_ip_lookup_success() -> Result<(), Box<dyn Error>> {
        let res = parse_or_resolve_ip("localhost")?;
        assert_eq!(res, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        Ok(())
    }

    #[test]
    fn parse_or_resolve_ip_lookup_fail() -> Result<(), Box<dyn Error>> {
        let res = parse_or_resolve_ip("blablabla");
        assert!(res.is_err());
        Ok(())
    }
}
