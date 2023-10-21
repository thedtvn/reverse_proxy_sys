mod obj;

#[macro_use]
extern crate lazy_static;

use reqwest::Url;
use tokio::sync::Mutex;
use std::convert::Infallible;
use std::net::SocketAddr;
use hyper::server::conn::AddrStream;
use hyper::body::Body;
use tokio::net::TcpStream;
use hyper::Server;
use hyper::upgrade::OnUpgrade;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Request, Response, StatusCode};
use obj::{ConfigF, Domain};
use wildmatch::WildMatch;
use regex::Regex;
use governor::{RateLimiter, Quota};
use governor::clock::SystemClock;
use governor::middleware::NoOpMiddleware;
use governor::state::InMemoryState;
use std::time::SystemTime;
use std::num::NonZeroU32;
use dashmap::DashMap;

static CONFIG_PATH: &str = "./config.yaml";

fn load_config() -> Result<ConfigF, ()> {
    let file_data = std::fs::read_to_string(CONFIG_PATH).unwrap();
    let config:ConfigF = serde_yaml::from_str(&file_data.as_str()).unwrap();
    updated_ratelimit(&config);
    return Ok(config);
}

fn updated_ratelimit(config: &ConfigF) {
    for i in config.domains.iter() {
        let config_ratelimit = i.1.rate_limit.clone();
        if config_ratelimit.is_none() { continue; }
        let r_cf = config_ratelimit.unwrap();
        let config_ratelimit = r_cf.split("/").collect::<Vec<&str>>();
        let str_limit = config_ratelimit.get(0).unwrap().parse::<u32>().unwrap();
        let str_mode = config_ratelimit.get(1).unwrap().to_ascii_lowercase();
        let limit = NonZeroU32::new(str_limit).unwrap();
        let turn = NonZeroU32::new(1).unwrap();
        let quota: Quota;
        if str_mode == "s" {
            quota = Quota::per_second(turn);
        } else if  str_mode == "m" {
            quota = Quota::per_minute(turn);
        } else if  str_mode == "h" {
            quota = Quota::per_minute(turn);
        } else {
            panic!("Mode '{}' is Invalid", str_mode);
        }
        let fquota = quota.allow_burst(limit);
        let rlm: RateLimiter<String, DashMap<String, InMemoryState>, SystemClock, NoOpMiddleware<SystemTime>> = RateLimiter::dashmap_with_clock(fquota, &SystemClock::default());
        CONFIG_RATELIMIT.insert(i.0.to_string(), rlm);
    }
}

lazy_static! {
    static ref CONFIG: Mutex<ConfigF> = {
        let config:ConfigF = load_config().unwrap();
        let m = Mutex::new(config);
        m
    };
    static ref CONFIG_RATELIMIT: DashMap<String, RateLimiter<String, DashMap<String, InMemoryState>, SystemClock, NoOpMiddleware<SystemTime>>> = {
        let m = DashMap::new();
        m
    };
}

/* 

static RATELIMIT: Lazy<Mutex<HashMap<String, Ratelimiter>>> = Lazy::new(|| {
    let m = HashMap::new();
    Mutex::new(m)
});

fn unixtime_now() -> i32 {
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    return since_the_epoch.as_secs() as i32;
}

async fn get_proxy_cache(key: String, token: i32, time: i32) -> Ratelimiter {
    let key_lock = RATELIMIT.lock().await;
    let val = key_lock.get(key.as_str());
    if val.is_none() {
        return a;
    }
    return val.unwrap();
}

async fn clear_cache() {
    loop {
        let check_time = unixtime_now();
        for i in RATELIMIT.lock().await.iter() {
            break;
        }
        sleep(Duration::from_secs(30)).await;
    }
}
*/


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
    let hnr = &req.headers().get("Host");
    if hnr.is_none() {
        let mut rt = Response::new(Body::empty());
        *rt.status_mut() = StatusCode::BAD_REQUEST;
        return Ok(rt); 
    }
    let host = hnr.unwrap().to_str().unwrap();
    let mut domain_config: &Domain = &Domain::default();
    let mut domain_key: String = "".to_string();
    let domain_list = &CONFIG.lock().await.domains;
    for i in domain_list {
        if check_config_value(i.0.to_string(), host.to_string()) {
            let fdomain = i.1.clone();
            domain_config = fdomain;
            domain_key = i.0.to_string();
            break;
        }
    }
    if domain_config == &Domain::default() {
        let mut rt = Response::new(Body::empty());
        *rt.status_mut() = StatusCode::BAD_GATEWAY;
        return Ok(rt); 
    }
    let out = CONFIG_RATELIMIT.get(&domain_key).unwrap().check_key(&format!("{}|{}", host, ipadd));
    if out.is_err() {
        println!("Ratelimit {}", ipadd);
        let mut rt = Response::new(Body::empty());
        *rt.status_mut() = StatusCode::TOO_MANY_REQUESTS;
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