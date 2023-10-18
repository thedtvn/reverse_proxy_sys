mod yaml_obj;

use reqwest::Url;
use tokio::sync::Mutex;
use std::collections::HashMap;
use std::convert::Infallible;
use tokio::time::{sleep, Duration};
use std::time::{SystemTime, UNIX_EPOCH};
use std::net::SocketAddr;
use hyper::server::conn::AddrStream;
use hyper::body::Body;
use tokio::net::TcpStream;
use hyper::Server;
use hyper::upgrade::OnUpgrade;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Request, Response, StatusCode};
use once_cell::sync::Lazy;
use yaml_obj::{ConfigF, Domain, ProxyInfo, CacheItem};
use wildmatch::WildMatch;
use regex::Regex;

static CONFIG_PATH: &str = "./config.yaml";

fn load_config() -> Result<ConfigF, ()> {
    let file_data = std::fs::read_to_string(CONFIG_PATH).unwrap();
    let config:ConfigF = serde_yaml::from_str(&file_data.as_str()).unwrap();
    return Ok(config);
}

static PROXY_CACHE: Lazy<Mutex<HashMap<String, CacheItem<bool>>>> = Lazy::new(|| {
    let m = HashMap::new();
    Mutex::new(m)
});


static CONFIG: Lazy<Mutex<ConfigF>> = Lazy::new(|| {
    let config:ConfigF = load_config().unwrap();
    let m = Mutex::new(config);
    m
});


fn unixtime_now() -> i32 {
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    return since_the_epoch.as_secs() as i32;
}


async fn set_proxy_cache(key: String, value: bool, expires: i32) {
    let expier = unixtime_now() + expires;
    PROXY_CACHE.lock().await.insert(key, CacheItem { value: value, expier: expier, default_expires: expires });
}

async fn get_proxy_cache(key: String) -> Option<bool> {
    let key_lock = PROXY_CACHE.lock().await;
    let val = key_lock.get(key.as_str());
    if val.is_none() {
        return None;
    }
    let data = val.unwrap();
    tokio::spawn(set_proxy_cache(key, data.value, data.default_expires));
    return Some(data.value);
}

async fn proxy_check(ip: &str) -> bool {
    let mut out = false;
    let caher = get_proxy_cache(ip.to_string()).await;
    if caher.is_some() {
        return caher.unwrap();
    }
    let body = reqwest::get(format!("https://proxycheck.io/v2/{}?risk=2&vpn=3", ip)).await;
    if body.is_err() { 
        tokio::spawn(set_proxy_cache(ip.to_string(), out, 60));
        return out;
    }
    let body = body.unwrap();
    let json = body.json::<ProxyInfo>().await;
    if json.is_err() { 
        tokio::spawn(set_proxy_cache(ip.to_string(), out, 60));
        return out;
    }
    let json = json.unwrap();
    if json.status != "ok" { return out; }
    let data = json.server.get(ip).unwrap();
    if data.proxy == "yes" { out = true }
    if data.vpn == "yes" { 
        if data.risk <= 33{
            out = false
        }
    }
    if data.risk > 66 {
        out = true  
    }
    tokio::spawn(set_proxy_cache(ip.to_string(), out, 60));
    return out;
}

/// Check Configuration Matches
fn check_config_value(var: String, value: String) -> bool {
    let mut split_var = var.split(" ").collect::<Vec<&str>>();
    if (split_var.len() as i32) < 2 {
        println!("Invalid `{}`", var);
        return false;
    }
    let mut return_var: bool = false;
    let mode = split_var.remove(0).to_ascii_lowercase();
    let var = split_var.remove(0);
    if mode == "eq" || mode == "!eq" {

        return_var = var == value;

    } else if mode == "sw" || mode == "!sw" {

        return_var = value.starts_with(&var);

    } else if mode == "ct" || mode == "!ct" {

        return_var = value.contains(&var);

    } else if mode == "ew" || mode == "!ew" {

        return_var = value.ends_with(&var);

    } else if mode == "wc" || mode == "!wc" {

        return_var =  WildMatch::new(var).matches(value.as_str());

    } else if mode == "regex" || mode == "!regex" {

        let reg = Regex::new(var);
        if reg.is_ok() {
            return_var =  reg.unwrap().is_match(value.as_str());
        } else {
            println!("Invalid Regex `{}`", var);
            return_var = false;
        }

    }
    if mode.starts_with("!") {
        return_var = !return_var;
    }
    return return_var;
}

async fn get_ip_address(req: hyper::HeaderMap, addr_stream: SocketAddr) -> String {
    let add_h = Url::parse(format!("http://{}", addr_stream.to_string()).as_str()).unwrap();
    let mut out_ip = add_h.host_str().unwrap().to_string();
    let cf_header = req.get("CF-Connecting-IP");
    let forward = req.get("X-Forwarded-For");
    if forward.is_some() {
        let mut list_ip = forward.unwrap().to_str().unwrap().to_string();
        list_ip = list_ip.replace(" ", "");
        out_ip = list_ip.split(",").next().unwrap_or(out_ip.as_str()).to_string();
    }
    if cf_header.is_some() {
        out_ip = cf_header.unwrap().to_str().unwrap().to_string()
    }
    return out_ip;
}


/// Our server HTTP handler to initiate HTTP upgrades.
async fn server_upgrade(mut req: Request<Body>, addr_stream: SocketAddr) -> Result<Response<Body>, hyper::Error> {
    let ipadd = get_ip_address(req.headers().clone(), addr_stream).await;
    if proxy_check(ipadd.as_str()).await {
        println!("Blocked {}", ipadd.to_string());
        let mut rt = Response::new(Body::from("Your IP address is on blacklisted."));
        *rt.status_mut() = StatusCode::FORBIDDEN;
        return Ok(rt); 
    }
    let hnr = &req.headers().get("Host");
    if hnr.is_none() {
        let mut rt = Response::new(Body::empty());
        *rt.status_mut() = StatusCode::BAD_REQUEST;
        return Ok(rt); 
    }
    let host = hnr.unwrap().to_str().unwrap();
    let mut domain_config: &Domain = &Domain::default();
    let domain_list = &CONFIG.lock().await.domains;
    for i in domain_list {
        if check_config_value(i.0.to_string(), host.to_string()) {
            let fdomain = i.1.clone();
            domain_config = fdomain;
            break;
        }
    }
    if domain_config == &Domain::default() {
        let mut rt = Response::new(Body::empty());
        *rt.status_mut() = StatusCode::BAD_GATEWAY;
        return Ok(rt); 
    }
    let addr = domain_config.taget.clone();
    let client_stream = TcpStream::connect(addr).await;
    if client_stream.is_err() {
        let mut rt = Response::new(Body::from("Gateway error"));
        *rt.status_mut() = StatusCode::BAD_GATEWAY;
        return Ok(rt); 
    }
    let (mut sender, conn) = hyper::client::conn::handshake(client_stream.unwrap()).await.unwrap();
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("Error in connection: {}", e);
        }
    });
    let req_upgraded = req.extensions_mut().remove::<OnUpgrade>();
    let mut ret = sender.send_request(req).await.unwrap();
    let ret_upgraded = ret.extensions_mut().remove::<OnUpgrade>();
    let (parts, body) = ret.into_parts();
    if parts.status == hyper::StatusCode::SWITCHING_PROTOCOLS {
        tokio::spawn(async move {
            let mut _requp = req_upgraded.unwrap().await.unwrap();
            let mut _retup = ret_upgraded.unwrap().await.unwrap();
            let _ = tokio::io::copy_bidirectional(&mut _requp, &mut _retup).await;
        }); 
    }
    let rt = Response::from_parts(parts, body);
    return Ok(rt);
}

/// Hot Reload Config File.
async fn hot_reload() {
    let mut old_config = tokio::fs::read_to_string(CONFIG_PATH).await.unwrap();
    loop {
        let new_config = tokio::fs::read_to_string(CONFIG_PATH).await.unwrap();
        if old_config != new_config {
            let config:serde_yaml::Result<ConfigF> = serde_yaml::from_str(&new_config.as_str());
            if config.is_ok() {
                *CONFIG.lock().await = config.unwrap();
                old_config = new_config;
                println!("Config updated");
            }
    
        }
        sleep(Duration::from_secs(10)).await;
    }
}

async fn clear_cache() {
    loop {
        let check_time = unixtime_now();
        for i in PROXY_CACHE.lock().await.iter() {
            if i.1.expier <= check_time {
                PROXY_CACHE.lock().await.remove(i.0.as_str());
            }
        }
        sleep(Duration::from_secs(30)).await;
    }
}

/// Wait for the CTRL+C signal
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
}

#[tokio::main]
async fn main() {
    let addr: SocketAddr = ([0, 0, 0, 0], 3001).into();
    *CONFIG.lock().await = load_config().unwrap();
    
    tokio::spawn(hot_reload());
    tokio::spawn(clear_cache());

    let make_service =
    make_service_fn(move |conn: &AddrStream|{
        let addr = conn.remote_addr();
         async move {
             let addr=addr.clone();
        Ok::<_, Infallible>(service_fn(move |req| server_upgrade(req, addr.clone())))
    }});

    let server = Server::bind(&addr).serve(make_service);

    println!("Listening on http://{}", addr);

    let graceful = server.with_graceful_shutdown(shutdown_signal());

    // Run this server for... forever!
    if let Err(e) = graceful.await {
        eprintln!("server error: {}", e);
    }
}