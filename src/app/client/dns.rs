use hickory_resolver::Resolver;
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::proto::rr::RData;
use std::sync::Arc;
use tokio::task::JoinSet;

const TOP_SUBDOMAINS: &[&str] = &[
    "www",
    "api",
    "test",
    "dev",
    "staging",
    "admin",
    "mail",
    "webmail",
    "blog",
    "vpn",
    "portal",
    "secure",
    "login",
    "app",
    "dashboard",
    "jenkins",
    "gitlab",
    "jira",
    "confluence",
    "auth",
    "sso",
    "db",
    "mysql",
    "beta",
    "demo",
    "sandbox",
    "uat",
    "ftp",
    "cpanel",
    "metrics",
    "grafana",
    "prometheus",
    "kibana",
    "api-dev",
    "api-v1",
    "api-v2",
    "m",
    "mobile",
    "cdn",
    "assets",
    "static",
    "images",
    "media",
    "files",
    "support",
    "help",
    "docs",
    "developer",
    "store",
    "shop",
];

pub async fn enumerate_subdomains(domain: &str) -> Vec<String> {
    let (config, _) = hickory_resolver::system_conf::read_system_conf().unwrap();
    let resolver = Arc::new(
        Resolver::builder_with_config(config, TokioRuntimeProvider::default())
            .build()
            .unwrap(),
    );

    let mut set = JoinSet::new();

    for &sub in TOP_SUBDOMAINS {
        let full_domain = format!("{}.{}", sub, domain);
        let resolver = Arc::clone(&resolver);

        set.spawn(async move {
            match resolver.lookup_ip(full_domain.as_str()).await {
                Ok(response) => {
                    let ips: Vec<String> = response
                        .iter()
                        .map(|ip: std::net::IpAddr| ip.to_string())
                        .collect();

                    if !ips.is_empty() {
                        Some(format!("{} -> [{}]", full_domain, ips.join(", ")))
                    } else {
                        None
                    }
                }
                Err(_) => None,
            }
        });
    }

    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(Some(found)) = res {
            results.push(found);
        }
    }

    results.sort();
    results
}

pub async fn resolve_dns(domain: &str) -> Vec<String> {
    let (config, _) = hickory_resolver::system_conf::read_system_conf().unwrap();
    let resolver = Resolver::builder_with_config(config, TokioRuntimeProvider::default())
        .build()
        .unwrap();

    let mut results = Vec::new();

    if let Ok(response) = resolver.lookup_ip(domain).await {
        let ips: Vec<String> = response.iter().map(|ip| ip.to_string()).collect();
        if !ips.is_empty() {
            results.push(format!("A/AAAA: {}", ips.join(", ")));
        }
    }

    if let Ok(response) = resolver.mx_lookup(domain).await {
        let mxs: Vec<String> = response
            .answers()
            .iter()
            .filter_map(|r| {
                if let RData::MX(mx) = &r.data {
                    Some(format!("{} (pref: {})", mx.exchange, mx.preference))
                } else {
                    None
                }
            })
            .collect();
        if !mxs.is_empty() {
            results.push(format!("MX: {}", mxs.join(", ")));
        }
    }

    if let Ok(response) = resolver.txt_lookup(domain).await {
        let txts: Vec<String> = response
            .answers()
            .iter()
            .filter_map(|r| {
                if let RData::TXT(txt) = &r.data {
                    Some(
                        txt.txt_data
                            .iter()
                            .map(|bytes| String::from_utf8_lossy(bytes).to_string())
                            .collect::<Vec<_>>()
                            .join(""),
                    )
                } else {
                    None
                }
            })
            .collect();
        if !txts.is_empty() {
            results.push(format!("TXT: {}", txts.join(", ")));
        }
    }

    if let Ok(response) = resolver.ns_lookup(domain).await {
        let nss: Vec<String> = response
            .answers()
            .iter()
            .filter_map(|r| {
                if let RData::NS(ns) = &r.data {
                    Some(ns.0.to_string())
                } else {
                    None
                }
            })
            .collect();
        if !nss.is_empty() {
            results.push(format!("NS: {}", nss.join(", ")));
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rosagroleasing() {
        let domain = "rosagroleasing.ru";
        println!("Testing resolve_dns on {}...", domain);
        let resolved = resolve_dns(domain).await;
        for res in resolved {
            println!("{}", res);
        }

        println!("\nTesting enumerate_subdomains on {}...", domain);
        let subs = enumerate_subdomains(domain).await;
        for sub in subs {
            println!("{}", sub);
        }
    }
}
