mod yaml_obj;

use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use std::net::SocketAddr;
use hyper::body::Body;
use tokio::net::TcpStream;
use hyper::Server;
use hyper::upgrade::OnUpgrade;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Request, Response, StatusCode};
use once_cell::sync::Lazy;
use yaml_obj::ConfigF;

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

/// Our server HTTP handler to initiate HTTP upgrades.
async fn server_upgrade(mut req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    println!("{:?}", CONFIG.lock().await);
    let addr = "localhost:8080";
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
        sleep(Duration::from_secs(1)).await;
    }
}


#[tokio::main]
async fn main() {
    let addr: SocketAddr = ([0, 0, 0, 0], 3001).into();
    *CONFIG.lock().await = load_config().unwrap();
    
    tokio::spawn(hot_reload());

    let make_service = make_service_fn(|_conn| async {
        Ok::<_, hyper::Error>(service_fn(|req| async {
            server_upgrade(req).await
        }))
    });

    let server = Server::bind(&addr).serve(make_service);

    println!("Listening on http://{}", addr);

    server.await.unwrap();
}