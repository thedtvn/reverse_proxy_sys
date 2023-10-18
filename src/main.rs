mod yaml_obj;

use tokio::sync::Mutex;
use std::convert::Infallible;
use tokio::time::{sleep, Duration};
use std::net::SocketAddr;
use hyper::body::Body;
use tokio::net::TcpStream;
use hyper::Server;
use hyper::upgrade::OnUpgrade;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Request, Response, StatusCode};
use once_cell::sync::Lazy;
use yaml_obj::{ConfigF, Domain};
use wildmatch::WildMatch;
use regex::Regex;

static CONFIG_PATH: &str = "./config.yaml";

fn load_config() -> Result<ConfigF, ()> {
    let file_data = std::fs::read_to_string(CONFIG_PATH).unwrap();
    let config:ConfigF = serde_yaml::from_str(&file_data.as_str()).unwrap();
    return Ok(config);
}

static CONFIG: Lazy<Mutex<ConfigF>> = Lazy::new(|| {
    let config:ConfigF = load_config().unwrap();
    let m = Mutex::new(config);
    m
});

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



/// Our server HTTP handler to initiate HTTP upgrades.
async fn server_upgrade(mut req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
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

    let make_service = make_service_fn(|_conn| async {
        // service_fn converts our function into a `Service`
        Ok::<_, Infallible>(service_fn(server_upgrade))
    });

    let server = Server::bind(&addr).serve(make_service);

    println!("Listening on http://{}", addr);

    let graceful = server.with_graceful_shutdown(shutdown_signal());

    // Run this server for... forever!
    if let Err(e) = graceful.await {
        eprintln!("server error: {}", e);
    }
}