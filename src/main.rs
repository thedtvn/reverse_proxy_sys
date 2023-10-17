mod yaml_obj;

use tokio::sync::Mutex;
use std::net::SocketAddr;
use hyper::body::Body;
use tokio::net::TcpStream;
use hyper::Server;
use hyper::upgrade::OnUpgrade;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Request, Response, StatusCode};
use once_cell::sync::Lazy;
use yaml_obj::Config;

fn load_config() -> Config {
    let file_data = std::fs::read_to_string("./config.yaml").unwrap();
    let config:Config = serde_yaml::from_str(&file_data.as_str()).unwrap();
    return config;
}

static _CONFIG: Lazy<Mutex<Config>> = Lazy::new(|| {
    let config = load_config();
    let m = Mutex::new(config);
    m
});

/// Our server HTTP handler to initiate HTTP upgrades.
async fn server_upgrade(mut req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
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


#[tokio::main]
async fn main() {
    let addr: SocketAddr = ([0, 0, 0, 0], 3001).into();
    *_CONFIG.lock().await = load_config();
    let make_service = make_service_fn(|_conn| async {
        Ok::<_, hyper::Error>(service_fn(|req| async {
            server_upgrade(req).await
        }))
    });

    let server = Server::bind(&addr).serve(make_service);

    println!("Listening on http://{}", addr);

    server.await.unwrap();
}