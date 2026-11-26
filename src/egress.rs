//! SSRF guard. The runner usually sits inside a private network and fetches user-supplied URLs, so each
//! run's hosts are resolved once, refused unless every address is public unicast, and the
//! run's HTTP client is pinned to those addresses: later DNS answers (rebinding) are never
//! consulted, any other host name fails to resolve, and redirects are not followed.

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
    /// Tests reach local fixtures over loopback; production never does.
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

    /// A client that reaches only the hosts of `urls`, at the addresses vetted now. No proxy,
    /// redirects, or retries, so every failure is reported instead of hidden.
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
            // Addresses stay out of the message: it would reveal what our resolver sees.
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

/// Resolver for names that were not vetted: there are none, by construction.
struct Refuse;

impl Resolve for Refuse {
    fn resolve(&self, name: Name) -> Resolving {
        let error = format!("{} was not vetted for this run", name.as_str());
        Box::pin(async move { Err(error.into()) })
    }
}

/// Public unicast only: refuses loopback, private (RFC 1918, ULA), link-local (including the
/// 169.254.169.254 metadata service), CGNAT, unspecified, multicast, broadcast, reserved,
/// benchmarking, documentation, and IPv6 ranges that embed or translate IPv4.
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
    !(a == 0 // "this network", including 0.0.0.0
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_documentation()
        || (a == 100 && (64..128).contains(&b)) // CGNAT 100.64.0.0/10
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments 192.0.0.0/24
        || (a == 198 && (b & 0xfe) == 18) // benchmarking 198.18.0.0/15
        || a >= 240) // reserved 240.0.0.0/4 and broadcast
}

/// Allowlist: global unicast 2000::/3, minus the special-purpose blocks inside it.
fn public_v6(ip: Ipv6Addr) -> bool {
    let [first, second, ..] = ip.segments();
    (first & 0xe000) == 0x2000
        && !(first == 0x2001 && second < 0x0200) // IETF protocol assignments 2001::/23
        && !(first == 0x2001 && second == 0x0db8) // documentation 2001:db8::/32
        && first != 0x2002 // 6to4, which embeds IPv4
        && (first & 0xfff0) != 0x3ff0 // documentation 3fff::/20
}

#[cfg(test)]
mod tests;
