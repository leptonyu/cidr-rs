use std::{
    collections::{btree_set::Iter, BTreeSet},
    fs::File,
    io::{BufRead, Read},
    net::{Ipv4Addr, Ipv6Addr},
};

use cfg_rs::*;
#[cfg(target_env = "musl")]
#[global_allocator]
//static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(FromConfig, Debug)]
pub struct Config {
    #[config(default = false)]
    reverse: bool,
    #[config(default = true)]
    merge: bool,
    exclude: Option<String>,
    prefix_v4: Option<u8>,
    prefix_v6: Option<u8>,
}

fn main() -> Result<(), ConfigError> {
    let config = init_args(Configuration::with_predefined_builder()).init()?;
    let conf: Config = config.get("")?;
    let mut list = SubnetList::default();
    // println!("{:?}", conf);
    list.read_stdin(conf.reverse, conf.exclude, conf.merge)?;
    for subnet in list.iter() {
        match subnet.net {
            Ok(net) => {
                if let Some(prefix) = conf.prefix_v4 {
                    if prefix <= 32 && prefix > subnet.mask {
                        let count: u32 = 1 << (prefix - subnet.mask);
                        for i in 0..count {
                            let sub = Subnet::new_v4(net + (i << (32 - prefix)), prefix);
                            println!("{}", sub.to_string());
                        }
                        continue;
                    }
                }
            }
            Err(net) => {
                if let Some(prefix) = conf.prefix_v6 {
                    if prefix <= 128 && prefix > subnet.mask {
                        let count: u128 = 1 << (prefix - subnet.mask);
                        for i in 0..count {
                            let sub = Subnet::new_v6(net + (i << (128 - prefix)), prefix);
                            println!("{}", sub.to_string());
                        }
                        continue;
                    }
                }
            }
        }
        println!("{}", subnet.to_string());
    }
    Ok(())
}

fn init_args(mut builder: PredefinedConfigurationBuilder) -> PredefinedConfigurationBuilder {
    for arg in std::env::args() {
        if arg.find("--") == Some(0) {
            builder = add_arg(&arg["--".len()..], builder);
        }
    }
    builder
}

fn add_arg(
    arg: &str,
    mut builder: PredefinedConfigurationBuilder,
) -> PredefinedConfigurationBuilder {
    if let Some(r) = arg.find('=') {
        let key = &arg[0..r];
        let val = &arg[r + 1..];
        builder = builder.set(
            key.replace('-', "_"),
            if val.is_empty() {
                "true".to_string()
            } else {
                val.to_owned()
            },
        );
    }
    builder
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
struct Subnet {
    net: Result<u32, u128>,
    mask: u8,
    tag: u8,
}

macro_rules! common_parse_fn {
    ($tp:ident, $tpa:ident , $mp:literal, $name: ident, $new: ident, $try_ip: ident) => {
        fn $name(
            v: &mut SubnetList,
            mut from: $tp,
            mut to: $tp,
            tag: Option<u8>,
        ) -> Result<(), ConfigError> {
            let mut mask = $mp;
            while from <= to {
                if from == to {
                    if from != 0 || mask > 0 {
                        from <<= ($mp - mask);
                    }
                    let mut item = Subnet::$new(from, mask);
                    if let Some(t) = tag {
                        item.tag = t;
                    }
                    v.insert(item);
                    break;
                }
                if from & 1 == 1 {
                    let mut item = Subnet::$new(from << ($mp - mask), mask);
                    if let Some(t) = tag {
                        item.tag = t;
                    }
                    v.insert(item);
                    from += 1;
                }
                if to & 1 == 0 {
                    let mut item = Subnet::$new(to << ($mp - mask), mask);
                    if let Some(t) = tag {
                        item.tag = t;
                    }
                    v.insert(item);
                    to -= 1;
                }
                while from & 1 == 0 && to & 1 == 1 {
                    mask -= 1;
                    from >>= 1;
                    to >>= 1;
                }
            }
            Ok(())
        }

        fn $try_ip(s: &str, mask: Option<u8>) -> Result<Option<Self>, ConfigError> {
            let mask = mask.unwrap_or($mp);
            let addr: $tpa = s.parse()?;
            let net: $tp = $tp::from(addr);
            let mk = $mp - mask;
            Ok(Some(Self::$new((net >> mk) << mk, mask)))
        }
    };
}
impl Subnet {
    fn new_v4(net: u32, mask: u8) -> Self {
        Self {
            net: Ok(net),
            mask,
            tag: 0,
        }
    }
    fn new_v6(net: u128, mask: u8) -> Self {
        Self {
            net: Err(net),
            mask,
            tag: 0,
        }
    }

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

    common_parse_fn!(u32, Ipv4Addr, 32, parse_ipv4_range, new_v4, try_ipv4);
    common_parse_fn!(u128, Ipv6Addr, 128, parse_ipv6_range, new_v6, try_ipv6);

    fn parse_range(s: &str, v: &mut SubnetList, tag: Option<u8>) -> Result<(), ConfigError> {
        let split: Vec<&str> = Self::prepare_str(s).split(',').take(2).collect();
        let from = Subnet::parse_subnet(split[0])?
            .ok_or(ConfigError::RefValueRecursiveError)?
            .net;
        let to = Subnet::parse_subnet(split[1])?
            .ok_or(ConfigError::RefValueRecursiveError)?
            .net;
        match (from, to) {
            (Ok(from), Ok(to)) => Self::parse_ipv4_range(v, from, to, tag),
            (Err(from), Err(to)) => Self::parse_ipv6_range(v, from, to, tag),
            _ => panic!("Error"),
        }
    }

    fn prepare_str(mut s: &str) -> &str {
        s = s.trim();
        if let Some(i) = s.find('#') {
            s = s[0..i].trim();
        }
        s
    }

    fn parse(s: &str, vec: &mut SubnetList, tag: Option<u8>) -> Result<(), ConfigError> {
        if s.contains(',') {
            return Self::parse_range(s, vec, tag);
        }
        if let Some(mut x) = Self::parse_subnet(s)? {
            if let Some(t) = tag {
                x.tag = t;
            }
            vec.insert(x);
        }
        Ok(())
    }

    pub fn contains(&self, target: &Self) -> bool {
        if self.mask > target.mask {
            return false;
        }
        if self.mask == 0 {
            return true;
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
    pub fn read_stdin(
        &mut self,
        reverse: bool,
        exclude: Option<String>,
        merge: bool,
    ) -> Result<(), ConfigError> {
        let stdin = std::io::stdin();
        let lines = stdin.lock().lines();
        for line in lines {
            Subnet::parse(&line?, self, None).ok();
        }
        self.shrink(true);
        if let Some(file) = exclude {
            let mut file = File::open(file)?;
            let mut buf = String::new();
            file.read_to_string(&mut buf)?;
            let mut new = SubnetList::default();
            for line in buf.lines() {
                Subnet::parse(line, &mut new, None).ok();
            }
            new.shrink(true);
            self.merge(new, merge);
        } else if reverse {
            let ret = self.gap();
            let _ = std::mem::replace(self, ret);
        }

        Ok(())
    }

    fn merge(&mut self, new: SubnetList, merge: bool) {
        for mut item in new.gap().0.into_iter() {
            item.tag = 1;
            self.insert(item);
        }
        self.shrink(merge);
    }

    pub fn insert(&mut self, subnet: Subnet) -> bool {
        self.0.insert(subnet)
    }

    pub fn shrink(&mut self, merge: bool) {
        let mut vec: Vec<Subnet> = vec![];
        let mut last: Option<Subnet> = None;
        for i in self.0.iter() {
            if let Some(l) = &mut last {
                if l.contains(i) {
                    let len = vec.len();
                    vec[len - 1].tag = if merge { 0 } else { 2 };
                    continue;
                }
            }
            last = Some(*i);
            merge_vec(&mut vec, *i);
        }
        self.0.clear();
        for i in vec {
            if i.tag == 0 {
                self.insert(i);
            }
        }
    }

    pub fn iter(&self) -> Iter<Subnet> {
        self.0.iter()
    }

    pub fn gap(&self) -> Self {
        let mut last_v4 = (0, u32::MAX);
        let mut last_v6 = (0, u128::MAX);
        let mut list = SubnetList::default();
        for item in self.iter() {
            match item.net {
                Ok(s) => {
                    if s > last_v4.0 {
                        let _ = Subnet::parse_ipv4_range(&mut list, last_v4.0, s - 1, None);
                    }
                    let x = s + (1 << (32 - item.mask));
                    if x > last_v4.0 {
                        last_v4.0 = x;
                    }
                }
                Err(s) => {
                    if s > last_v6.0 {
                        let _ = Subnet::parse_ipv6_range(&mut list, last_v6.0, s - 1, None);
                    }
                    let x = s + (1 << (128 - item.mask));
                    if x > last_v6.0 {
                        last_v6.0 = x;
                    }
                }
            }
        }
        if last_v4.0 < last_v4.1 {
            let _ = Subnet::parse_ipv4_range(&mut list, last_v4.0, last_v4.1, None);
        }
        if last_v6.0 < last_v6.1 {
            let _ = Subnet::parse_ipv6_range(&mut list, last_v6.0, last_v6.1, None);
        }
        list
    }
}

fn merge_vec(vec: &mut Vec<Subnet>, mut i: Subnet) {
    while let Some(mut l) = vec.pop() {
        if l.tag == i.tag && l.is_next(&i) {
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
        set.shrink(true);
        println!("------ Shrink List");
        for x in set.iter() {
            println!("{}", x.to_string());
        }
        Ok(())
    }

    #[test]
    fn range_test() -> Result<(), ConfigError> {
        let mut list = SubnetList::default();
        Subnet::parse("223.255.229.0,223.255.230.255,", &mut list, None)?;
        Subnet::parse(
            "2c0f:fc00:b011::,2c0f:fc00:b011:ffff:ffff:ffff:ffff:ffff,",
            &mut list,
            None,
        )?;
        for x in list.iter() {
            println!("{}", x.to_string());
        }
        Ok(())
    }

    #[test]
    fn test_gap() -> Result<(), ConfigError> {
        let mut list = SubnetList::default();
        insert!(list. "0.0.0.0/8");
        insert!(list. "1.0.0.0/24");
        print_list(&list.gap());
        Ok(())
    }

    fn print_list(list: &SubnetList) {
        for x in list.iter() {
            println!("{}", x.to_string());
        }
    }

    #[test]
    fn test_merge() -> Result<(), ConfigError> {
        let mut list = SubnetList::default();
        insert!(list. "1.1.1.0/24");
        insert!(list. "1.1.3.0/24");
        insert!(list. "2.2.2.0/24");
        insert!(list. "2.3.2.0/24");
        list.shrink(true);
        print_list(&list);
        let mut excl = SubnetList::default();
        insert!(excl. "0.0.0.0/8");
        insert!(excl. "1.1.2.0/24");
        excl.shrink(true);
        println!("------ Gap List");
        print_list(&excl.gap());
        list.merge(excl, false);
        println!("------ Mergr List");
        print_list(&list);
        Ok(())
    }
}
