use super::*;
use std::net::{Ipv4Addr, Ipv6Addr};

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

#[test]
fn only_public_unicast_addresses_pass() {
    for blocked in [
        "0.0.0.0",
        "0.1.2.3",
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "172.31.255.255",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "100.127.255.255",
        "192.0.0.1",
        "192.0.2.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "240.0.0.1",
        "255.255.255.255",
        "::",
        "::1",
        "fc00::1",
        "fd00:ec2::254",
        "fe80::1",
        "ff02::1",
        "2001:db8::1",
        "::ffff:10.0.0.1",
        "::ffff:169.254.169.254",
        "64:ff9b::a00:1",
        "2002:a00:1::",
        "2001::1",
    ] {
        assert!(!public(ip(blocked)), "{blocked} must be refused");
    }
    for allowed in [
        "8.8.8.8",
        "1.1.1.1",
        "172.32.0.1",
        "100.128.0.1",
        "2606:4700::1111",
        "::ffff:8.8.8.8",
    ] {
        assert!(public(ip(allowed)), "{allowed} must pass");
    }
    assert!(public(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))));
    assert!(!public(IpAddr::V6(Ipv6Addr::LOCALHOST)));
}

fn urls(list: &[&str]) -> Vec<Url> {
    list.iter().map(|url| Url::parse(url).unwrap()).collect()
}

#[tokio::test]
async fn refuses_non_public_hosts_without_naming_their_addresses() {
    for url in [
        "https://127.0.0.1/mcp",
        "https://169.254.169.254/latest/meta-data",
        "https://[::ffff:10.0.0.1]/",
        "https://localhost/mcp",
    ] {
        let error = Egress::new()
            .client(
                &urls(&[url]),
                Duration::from_secs(1),
                Duration::from_secs(1),
            )
            .await
            .err()
            .unwrap();
        assert!(
            error.ends_with("resolves to a non-public address"),
            "{url}: {error}"
        );
        if url.contains("localhost") {
            assert!(
                !error.contains("127.0.0.1") && !error.contains("::1"),
                "{error}"
            );
        }
    }
}

#[tokio::test]
async fn client_reaches_only_vetted_hosts_at_vetted_addresses() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = axum::Router::new().fallback(|| async { "pinned" });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let client = Egress::loopback()
        .client(
            &urls(&[&format!("http://localhost:{port}/")]),
            Duration::from_secs(1),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    let body = client
        .get(format!("http://localhost:{port}/"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(body, "pinned");
    let error = client
        .get(format!("http://unvetted.invalid:{port}/"))
        .send()
        .await
        .unwrap_err();
    assert!(error.is_connect(), "{error:?}");
}
