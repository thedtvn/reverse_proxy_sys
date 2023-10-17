use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct Config {
    nodes: HashMap<String, Domain>,
}

#[derive(Serialize, Deserialize)]
pub struct Domain {
    #[serde(rename = "taget")]
    taget: String,

    #[serde(rename = "ipcheck")]
    ipcheck: Option<bool>,
}