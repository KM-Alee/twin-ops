use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetInterface {
    pub name: String,
    pub addresses: Vec<IpAddr>,
    pub has_default_route: bool,
}

/// Joins `/proc/net/{route,fib_trie,if_inet6,ipv6_route}` into per-interface local
/// addresses and default-route ownership without netlink or ioctls. IPv4 local
/// addresses come from `fib_trie` LOCAL entries and are attributed to the interface
/// whose most specific route covers them; loopback addresses belong to `lo`.
pub fn parse_interfaces(
    route: &str,
    fib_trie: &str,
    if_inet6: &str,
    ipv6_route: &str,
) -> Vec<NetInterface> {
    let mut interfaces: Vec<NetInterface> = Vec::new();

    let routes = parse_ipv4_routes(route);
    for (name, destination, mask) in &routes {
        let interface = entry(&mut interfaces, name);
        if *destination == 0 && *mask == 0 {
            interface.has_default_route = true;
        }
    }
    for address in ipv4_local_addresses(fib_trie) {
        let value = u32::from(address);
        let owner = if address.is_loopback() {
            Some("lo")
        } else {
            routes
                .iter()
                .filter(|(_, destination, mask)| *mask != 0 && value & mask == *destination)
                .max_by_key(|(_, _, mask)| mask.count_ones())
                .map(|(name, _, _)| name.as_str())
        };
        if let Some(name) = owner {
            let interface = entry(&mut interfaces, name);
            let address = IpAddr::V4(address);
            if !interface.addresses.contains(&address) {
                interface.addresses.push(address);
            }
        }
    }
    for line in if_inet6.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let (Some(hex), Some(name)) = (fields.first(), fields.get(5)) else {
            continue;
        };
        if let Some(address) = parse_ipv6_hex(hex) {
            entry(&mut interfaces, name)
                .addresses
                .push(IpAddr::V6(address));
        }
    }
    for line in ipv6_route.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 10 {
            continue;
        }
        let is_default = fields[0].chars().all(|c| c == '0') && fields[1] == "00";
        let name = fields[9];
        if is_default && name != "lo" {
            entry(&mut interfaces, name).has_default_route = true;
        }
    }
    interfaces.sort_by(|a, b| a.name.cmp(&b.name));
    interfaces
}

fn entry<'a>(interfaces: &'a mut Vec<NetInterface>, name: &str) -> &'a mut NetInterface {
    let index = match interfaces.iter().position(|i| i.name == name) {
        Some(index) => index,
        None => {
            interfaces.push(NetInterface {
                name: name.to_string(),
                addresses: Vec::new(),
                has_default_route: false,
            });
            interfaces.len() - 1
        }
    };
    &mut interfaces[index]
}

/// `(interface, destination, mask)` with host-order values.
fn parse_ipv4_routes(route: &str) -> Vec<(String, u32, u32)> {
    route
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            let name = fields.first()?;
            let destination = u32::from_str_radix(fields.get(1)?, 16).ok()?;
            let mask = u32::from_str_radix(fields.get(7)?, 16).ok()?;
            Some((
                name.to_string(),
                u32::from_be(destination),
                u32::from_be(mask),
            ))
        })
        .collect()
}

fn ipv4_local_addresses(fib_trie: &str) -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    let mut last: Option<Ipv4Addr> = None;
    for line in fib_trie.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("|-- ") {
            last = rest.trim().parse().ok();
        } else if trimmed.starts_with("/32 host LOCAL") {
            if let Some(address) = last {
                if !out.contains(&address) {
                    out.push(address);
                }
            }
        }
    }
    out
}

fn parse_ipv6_hex(hex: &str) -> Option<Ipv6Addr> {
    if hex.len() != 32 {
        return None;
    }
    let mut octets = [0u8; 16];
    for (index, octet) in octets.iter_mut().enumerate() {
        *octet = u8::from_str_radix(hex.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(Ipv6Addr::from(octets))
}
