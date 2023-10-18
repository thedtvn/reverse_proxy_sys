use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct ConfigF {
    pub domains: HashMap<String, Domain>,
}

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Domain {
    #[serde(rename = "taget")]
    pub taget: String,
/*
    #[serde(rename = "ip_proxy")]
    pub ip_proxy: Option<bool>,

    #[serde(rename = "rate_limit")]
    pub rate_limit: Option<Vec<i32>>,
*/
}

#[derive(Debug, Deserialize)]
pub struct ProxyInfo {
    pub status: String,
    #[serde(flatten)]
    pub server: HashMap<String, Proxy>,
}

#[derive(Debug, Deserialize)]
pub struct Proxy {
    pub vpn: String,
    pub proxy: String,
    pub risk: u32
}

pub struct CacheItem<T> {
    pub value: T,
    pub expier: i32,
    pub default_expires: i32
}
