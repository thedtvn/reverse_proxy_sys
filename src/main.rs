mod obj;

#[macro_use]
extern crate lazy_static;
#[macro_use]                                                                                         
extern crate dlopen_derive;                                                                          

use clap::Parser;
use dlopen::wrapper::{Container, WrapperApi};
use url::Url;
use tokio::sync::Mutex;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use hyper::server::conn::AddrStream;
use hyper::body::Body;
use tokio::net::TcpStream;
use hyper::Server;
use hyper::upgrade::OnUpgrade;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Request, Response, StatusCode};
use obj::{ConfigF, ResponsePlugin, RequestPlugin};
use wildmatch::WildMatch;
use regex::Regex;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, value_name = "FILE", default_value = "config.yaml")]
    config: PathBuf,

    #[arg(short, long, value_name = "DIR", default_value = "plugins")]
    plugins_dir: PathBuf,
}



#[derive(WrapperApi)]
struct Plugin {
    on_request: fn(request: &mut RequestPlugin) -> (),
    on_response: fn(response: &mut ResponsePlugin) -> ()
}

fn load_config() -> Result<ConfigF, ()> {
    if !&ARGS_CONFIG.config.is_file() {
        println!("'{}' is not a config file", ARGS_CONFIG.config.display());
        std::process::exit(1);
    }
    let file_data = std::fs::read_to_string(&ARGS_CONFIG.config).unwrap();
    let config:ConfigF = serde_yaml::from_str(&file_data.as_str()).unwrap_or_else(|_| {
        println!("Failed to load config file '{}'", ARGS_CONFIG.config.display());
        std::process::exit(1);
    });
    return Ok(config);
}

lazy_static! {
    static ref CONFIG: Mutex<ConfigF> = {
        let config:ConfigF = load_config().unwrap();
        let m = Mutex::new(config);
        m
    };
    static ref ARGS_CONFIG: Args = Args::parse();
    static ref PLUGINS: HashMap<String, Container<Plugin>> = load_plugins();
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
    if cf_header.is_some() {
        out_ip = cf_header.unwrap().to_str().unwrap().to_string()
    }else if forward.is_some() {
        let mut list_ip = forward.unwrap().to_str().unwrap().to_string();
        list_ip = list_ip.replace(" ", "");
        out_ip = list_ip.split(",").next().unwrap_or(out_ip.as_str()).to_string();
    }
    return out_ip;
}

fn load_plugins() -> HashMap<String, Container<Plugin>> {
    let mut plugins = HashMap::new();
    for file in std::fs::read_dir(&ARGS_CONFIG.plugins_dir).unwrap() {
        let path = file.unwrap().path();
        let os_name =  std::env::consts::OS;
        let file_extension;
        if os_name == "windows" {
            file_extension = "dll";
        } else if os_name == "macos" {
            file_extension = "dylib";
        } else if os_name == "linux" {
            file_extension = "so";
        } else {
            file_extension = "";
        }
        if path.is_file() && path.extension().unwrap().to_str().unwrap() == file_extension {
            let name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let plugin_api_wrapper: Container<Plugin> = unsafe { Container::load(path) }.unwrap();
            plugins.insert(name, plugin_api_wrapper);
        }
    }
    return plugins;
}

/// Our server HTTP handler to initiate HTTP upgrades.
async fn server_upgrade(req: Request<Body>, addr_stream: SocketAddr) -> Result<Response<Body>, hyper::Error> {
    let ipadd = get_ip_address(req.headers().clone(), addr_stream).await;
    let hnr = &req.headers().get("Host");
    if hnr.is_none() {
        let mut rt = Response::new(Body::empty());
        *rt.status_mut() = StatusCode::BAD_REQUEST;
        return Ok(rt); 
    }
    let host = hnr.unwrap().to_str().unwrap();
    let mut domain_key = None;
    let domain_list = &CONFIG.lock().await.domains;
    for i in domain_list {
        if check_config_value(i.0.to_string(), host.to_string()) {
            domain_key = Some(i.1);
            break;
        }
    }
    let addr = if domain_key.is_some() {
        Some(domain_key.unwrap().taget.clone())
    } else {
        None
    };
    let mut cache = HashMap::new();
    let (parts, body) = req.into_parts();
    let mut req_plugin = RequestPlugin::new(parts, body, addr, cache);
    for x in PLUGINS.values() {
        x.on_request(&mut req_plugin);
    }
    let r_addr = req_plugin.get_foword_to();
    cache = req_plugin.get_cache();
    let mut req = req_plugin.to_request();
    if r_addr.is_none() {
        return Ok(Response::builder().status(StatusCode::NOT_IMPLEMENTED).body(Body::empty()).unwrap());
    }
    let addr = r_addr.unwrap();
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
    req.headers_mut().remove("x-forwarded-for");
    req.headers_mut().append("x-forwarded-for", ipadd.parse().unwrap());
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
    let mut response_plugin = ResponsePlugin::new(parts, body, cache);
    for x in PLUGINS.values() {
        x.on_response(&mut response_plugin);
    }
    return Ok(response_plugin.to_response());
}


/// Wait for the CTRL+C signal
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
}

#[tokio::main]
async fn main() {
    *CONFIG.lock().await = load_config().unwrap();
    let addrs_iter = CONFIG.lock().await.bind.to_socket_addrs();
    if addrs_iter.is_err() {
        panic!("{}", addrs_iter.unwrap_err());
    }
    let addr = addrs_iter.unwrap().next().unwrap();
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