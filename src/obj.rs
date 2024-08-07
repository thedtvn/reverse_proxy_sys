use hyper::{http::{request::Parts as RequestParts, response::Parts as ResponseParts}, Body, Response, Request};
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

pub struct RequestPlugin {
    pub parts: RequestParts,
    pub body: Body,
    pub cache: HashMap<String, String>,
    pub foword_to: Option<String>,
}

impl RequestPlugin {
    pub fn new(parts: RequestParts, body: Body, foword_to: Option<String>, cache: HashMap<String, String>) -> Self {
        Self {
            parts,
            body,
            foword_to,
            cache,
        }
    }

    pub fn to_request(self) -> Request<Body> {
        Request::from_parts(self.parts, self.body)
    }

    pub fn get_foword_to(&self) -> Option<String> {
        self.foword_to.clone()
    }

    pub fn get_cache(&self) -> HashMap<String, String> {
        self.cache.clone()
    }
}

pub struct ResponsePlugin {
    pub parts: ResponseParts,
    pub body: Body,
    pub cache: HashMap<String, String>,
}

impl ResponsePlugin {
    pub fn new(parts: ResponseParts, body: Body, cache: HashMap<String, String>) -> Self {
        Self {
            parts,
            body,
            cache,
        }
    }

    pub fn to_response(self) -> Response<Body> {
        Response::from_parts(self.parts, self.body)
    }
}