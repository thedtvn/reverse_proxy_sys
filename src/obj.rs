use serde::{Serialize, Deserialize};
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
    pub plugins: Option<Vec<String>>,
}