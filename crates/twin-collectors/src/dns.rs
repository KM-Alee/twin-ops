use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::time::Duration;

use thiserror::Error;
use tokio::runtime::{Builder, Handle};
use tokio::time::{timeout_at, Instant};

pub type DnsAnswers = BTreeMap<String, Result<Vec<IpAddr>, DnsError>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DnsError {
    #[error("DNS lookup unavailable")]
    Unavailable,
    #[error("DNS lookup deadline exceeded")]
    Timeout,
    #[error("DNS hostname limit exceeded")]
    Limit,
    #[error("DNS lookup returned no addresses")]
    Empty,
}

pub trait DnsReader {
    fn resolve(&self, hosts: &BTreeSet<String>) -> DnsAnswers;
}

pub struct SystemDnsReader;

impl DnsReader for SystemDnsReader {
    fn resolve(&self, hosts: &BTreeSet<String>) -> DnsAnswers {
        if hosts.is_empty() {
            return BTreeMap::new();
        }
        if Handle::try_current().is_ok() {
            return unavailable(hosts);
        }
        let Ok(runtime) = Builder::new_current_thread().enable_all().build() else {
            return unavailable(hosts);
        };
        let answers = runtime.block_on(resolve_async(hosts));
        // The OS resolver cannot be cancelled. Do not wait indefinitely for its blocking task.
        runtime.shutdown_background();
        answers
    }
}

pub async fn resolve_async(hosts: &BTreeSet<String>) -> DnsAnswers {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut answers = BTreeMap::new();
    for (index, host) in hosts.iter().enumerate() {
        let result = if index >= 32 {
            Err(DnsError::Limit)
        } else if Instant::now() >= deadline {
            Err(DnsError::Timeout)
        } else {
            match timeout_at(deadline, tokio::net::lookup_host((host.as_str(), 0))).await {
                Ok(Ok(addresses)) => {
                    let addresses: BTreeSet<_> = addresses.map(|address| address.ip()).collect();
                    if addresses.is_empty() {
                        Err(DnsError::Empty)
                    } else {
                        Ok(addresses.into_iter().collect())
                    }
                }
                Ok(Err(_)) => Err(DnsError::Unavailable),
                Err(_) => Err(DnsError::Timeout),
            }
        };
        answers.insert(host.clone(), result);
    }
    answers
}

fn unavailable(hosts: &BTreeSet<String>) -> DnsAnswers {
    hosts
        .iter()
        .map(|host| (host.clone(), Err(DnsError::Unavailable)))
        .collect()
}
