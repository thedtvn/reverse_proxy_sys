use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConfigF {
    nodes: HashMap<String, Domain>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Domain {
    #[serde(rename = "taget")]
    taget: String,

    #[serde(rename = "ipcheck")]
    ipcheck: Option<bool>,
}