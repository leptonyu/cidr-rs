use std::{
    collections::{btree_set::Iter, BTreeSet},
    io::BufRead,
    net::{Ipv4Addr, Ipv6Addr},
};

use cfg_rs::*;

fn main() -> Result<(), ConfigError> {
    let mut list = SubnetList::default();
    list.read_stdin()?;
    list.shrink();
    for subnet in list.iter() {
        println!("{}", subnet.to_string());
    }
    Ok(())
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
struct Subnet {
    net: Result<u32, u128>,
    mask: u8,
}

impl Subnet {
    fn parse_subnet(mut s: &str) -> Result<Option<Self>, ConfigError> {
        s = Self::prepare_str(s);
        if s.is_empty() {
            return Ok(None);
        }
        let mut mask: Option<u8> = None;
        if let Some(i) = s.find('/') {
            mask = Some((s[i + 1..]).parse()?);
            s = &s[0..i];
        }
        Self::try_ipv4(s, mask).or_else(|_| Self::try_ipv6(s, mask))
    }

    fn try_ipv4(s: &str, mask: Option<u8>) -> Result<Option<Self>, ConfigError> {
        let mask = mask.unwrap_or(32);
        let addr: Ipv4Addr = s.parse()?;
        let net = u32::from(addr);
        let mk = 32 - mask;
        Ok(Some(Self {
            net: Ok((net >> mk) << mk),
            mask,
        }))
    }

    fn try_ipv6(s: &str, mask: Option<u8>) -> Result<Option<Self>, ConfigError> {
        let mask = mask.unwrap_or(128);
        let addr: Ipv6Addr = s.parse()?;
        let net: u128 = u128::from(addr);
        let mk = 128 - mask;
        Ok(Some(Self {
            net: Err((net >> mk) << mk),
            mask,
        }))
    }

    // fn parse_range(s: &str, v: &mut SubnetList) -> Result<(), ConfigError> {
    //     let split: Vec<&str> = Self::prepare_str(s).split('|').take(2).collect();
    //     let mut from = Subnet::parse_subnet(split[0])?
    //         .ok_or(ConfigError::RefValueRecursiveError)?
    //         .net;
    //     let mut to = Subnet::parse_subnet(split[1])?
    //         .ok_or(ConfigError::RefValueRecursiveError)?
    //         .net;
    //     let mut mask: u8 = 32;
    //     while from <= to {
    //         if from == to {
    //             v.insert(Self {
    //                 net: from << (32 - mask),
    //                 mask,
    //             });
    //             break;
    //         }
    //         if from & 1 == 1 {
    //             v.insert(Self {
    //                 net: from << (32 - mask),
    //                 mask,
    //             });
    //             from += 1;
    //         }
    //         if to & 1 == 0 {
    //             v.insert(Self {
    //                 net: to << (32 - mask),
    //                 mask,
    //             });
    //             to -= 1;
    //         }
    //         while from & 1 == 0 && to & 1 == 1 {
    //             mask -= 1;
    //             from >>= 1;
    //             to >>= 1;
    //         }
    //     }
    //     Ok(())
    // }

    fn prepare_str(mut s: &str) -> &str {
        s = s.trim();
        if let Some(i) = s.find('#') {
            s = s[0..i].trim();
        }
        s
    }

    fn parse(s: &str, vec: &mut SubnetList) -> Result<(), ConfigError> {
        // if s.contains('|') {
        //     Self::parse_range(s, vec)?
        // } else
        if let Some(x) = Self::parse_subnet(s)? {
            vec.insert(x);
        }
        Ok(())
    }

    pub fn contains(&self, target: &Self) -> bool {
        if self.mask > target.mask {
            return false;
        }
        match (self.net, target.net) {
            (Ok(a), Ok(b)) => {
                let n = 32 - self.mask;
                a == (b >> n) << n
            }
            (Err(a), Err(b)) => {
                let n = 128 - self.mask;
                a == (b >> n) << n
            }
            _ => false,
        }
    }

    pub fn is_next(&self, target: &Self) -> bool {
        if self.mask != target.mask {
            return false;
        }
        match (self.net, target.net) {
            (Ok(a), Ok(b)) => {
                let n = 32 - self.mask;
                let v = a >> n;
                v & 1 == 0 && (a >> n) + 1 == (b >> n)
            }
            (Err(a), Err(b)) => {
                let n = 128 - self.mask;
                let v = a >> n;
                v & 1 == 0 && (a >> n) + 1 == (b >> n)
            }
            _ => false,
        }
    }
}

impl ToString for Subnet {
    fn to_string(&self) -> String {
        match self.net {
            Ok(net) => {
                format!("{}/{}", Ipv4Addr::from(net), self.mask)
            }
            Err(net) => {
                format!("{}/{}", Ipv6Addr::from(net), self.mask)
            }
        }
    }
}

#[derive(Default)]
struct SubnetList(BTreeSet<Subnet>);

impl SubnetList {
    pub fn read_stdin(&mut self) -> Result<(), ConfigError> {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            Subnet::parse(&line?, self).ok();
        }
        Ok(())
    }

    pub fn insert(&mut self, subnet: Subnet) -> bool {
        self.0.insert(subnet)
    }

    pub fn shrink(&mut self) {
        let mut vec = vec![];
        let mut last: Option<Subnet> = None;
        for i in self.0.iter() {
            if let Some(l) = &last {
                if l.contains(i) {
                    continue;
                }
            }
            last = Some(*i);
            merge_vec(&mut vec, *i);
        }
        self.0.clear();
        self.0.extend(vec);
    }

    pub fn iter(&self) -> Iter<Subnet> {
        self.0.iter()
    }
}

fn merge_vec(vec: &mut Vec<Subnet>, mut i: Subnet) {
    while let Some(mut l) = vec.pop() {
        if l.is_next(&i) {
            l.mask -= 1;
            i = l;
        } else {
            vec.push(l);
            break;
        }
    }
    vec.push(i);
}

#[cfg(test)]
mod tests {

    use cfg_rs::ConfigError;

    use crate::{Subnet, SubnetList};

    macro_rules! assert_empty {
        ($source:expr) => {
            assert_eq!(true, Subnet::parse_subnet($source)?.is_none());
        };
    }

    macro_rules! assert_subnet {
        ($source:expr => $target:expr) => {
            assert_eq!($target, Subnet::parse_subnet($source)?.unwrap().to_string());
        };
    }

    #[test]
    fn addr_test() -> Result<(), ConfigError> {
        assert_empty!("#");
        assert_empty!("#127.0.0.1");
        assert_empty!("");
        assert_subnet!("127.0.0.1   ####hello" => "127.0.0.1/32");
        assert_subnet!("127.0.0.1" => "127.0.0.1/32");
        assert_subnet!("127.0.0.1/31" => "127.0.0.0/31");
        assert_subnet!("127.0.0.1/8" => "127.0.0.0/8");
        assert_subnet!("127.0.0.1/7" => "126.0.0.0/7");
        assert_subnet!("0::1/128" => "::1/128");
        Ok(())
    }

    macro_rules! insert {
        ($set:ident.$x:expr) => {
            if let Some(v) = Subnet::parse_subnet($x)? {
                $set.insert(v);
            }
        };
    }

    #[test]
    fn addr_sort_test() -> Result<(), ConfigError> {
        let mut set = SubnetList::default();
        insert!(set. "128.0.0.0/6");
        insert!(set. "127.0.0.1/7");
        insert!(set. "127.0.0.1/7");
        insert!(set. "127.0.0.1/8");
        insert!(set. "127.0.0.1/9");
        insert!(set. "127.0.0.1/6");
        insert!(set. "::1/10");
        insert!(set. "::1/10");
        println!("------ Full List");
        for x in set.iter() {
            println!("{}", x.to_string());
        }
        set.shrink();
        println!("------ Shrink List");
        for x in set.iter() {
            println!("{}", x.to_string());
        }
        Ok(())
    }

    // #[test]
    // fn range_test() -> Result<(), ConfigError> {
    //     let mut list = SubnetList::default();
    //     Subnet::parse("223.255.229.0|223.255.230.255|", &mut list)?;
    //     for x in list.iter() {
    //         println!("{}", x.to_string());
    //     }
    //     Ok(())
    // }
}
