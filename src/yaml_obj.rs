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

/*    #[serde(rename = "ipcheck")]
    pub ipcheck: Option<bool>,*/
}