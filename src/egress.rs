use reqwest::{
    Url,
    dns::{Name, Resolve, Resolving},
};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

pub struct Egress {
    #[cfg(test)]
    loopback: bool,
}

impl Egress {
    pub fn new() -> Self {
        Self {
            #[cfg(test)]
            loopback: false,
        }
    }

    #[cfg(test)]
    pub fn loopback() -> Self {
        Self { loopback: true }
    }

    fn allows(&self, ip: IpAddr) -> bool {
        #[cfg(test)]
        if self.loopback && ip.is_loopback() {
            return true;
        }
        public(ip)
    }

    pub async fn client(
        &self,
        urls: &[Url],
        connect: Duration,
        timeout: Duration,
    ) -> Result<reqwest::Client, String> {
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(connect)
            .timeout(timeout)
            .dns_resolver(Arc::new(Refuse));
        for url in urls {
            let port = url.port_or_known_default().unwrap_or_default();
            let host = url.host_str().ok_or_else(|| format!("{url} has no host"))?;
            let literal = host.trim_start_matches('[').trim_end_matches(']').parse();
            let (domain, addrs): (_, Vec<SocketAddr>) = match literal {
                Ok(ip) => (None, vec![SocketAddr::new(ip, port)]),
                Err(_) => {
                    let addrs = tokio::net::lookup_host((host, port))
                        .await
                        .map_err(|_| format!("{host} does not resolve"))?;
                    (Some(host), addrs.collect())
                }
            };
            if addrs.is_empty() || !addrs.iter().all(|addr| self.allows(addr.ip())) {
                return Err(format!("{host} resolves to a non-public address"));
            }
            if let Some(domain) = domain {
                builder = builder.resolve_to_addrs(domain, &addrs);
            }
        }
        builder
            .build()
            .map_err(|_| "the HTTP client cannot be built".to_owned())
    }
}

struct Refuse;

impl Resolve for Refuse {
    fn resolve(&self, name: Name) -> Resolving {
        let error = format!("{} was not vetted for this run", name.as_str());
        Box::pin(async move { Err(error.into()) })
    }
}

pub fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(mapped) => public_v4(mapped),
            None => public_v6(ip),
        },
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_documentation()
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (b & 0xfe) == 18)
        || a >= 240)
}

fn public_v6(ip: Ipv6Addr) -> bool {
    let [first, second, ..] = ip.segments();
    (first & 0xe000) == 0x2000
        && !(first == 0x2001 && second < 0x0200)
        && !(first == 0x2001 && second == 0x0db8)
        && first != 0x2002
        && (first & 0xfff0) != 0x3ff0
}

#[cfg(test)]
mod tests;
