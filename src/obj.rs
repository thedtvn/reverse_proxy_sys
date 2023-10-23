use serde::{Serialize, Deserialize};
use std::num::NonZeroU32;
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ConfigF {
    pub bind: String,
    #[serde(flatten)]
    pub domains: HashMap<String, Domain>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Domain {
    #[serde(rename = "taget")]
    pub taget: String,
    #[serde(rename = "rate_limit")]
    pub rate_limit: Option<RateLimit>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RateLimit {
    pub per: String,
    pub limit: NonZeroU32,
    pub burst: Option<NonZeroU32>,
}