mod colormap;
mod delaunator;
mod grid_mesher;
mod mesh_service;
mod pcd;
mod proto;
mod resample;
mod scan;
mod scan_service;
mod service;
mod statistics;

use proto::farisland::threed::v1::mesh_service_server::MeshServiceServer;
use proto::farisland::threed::v1::point_cloud_service_server::PointCloudServiceServer;
use proto::farisland::threed::v1::three_d_scan_service_server::ThreeDScanServiceServer;
use mesh_service::MeshServiceImpl;
use scan_service::ThreeDScanServiceImpl;
use service::PointCloudServiceImpl;
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::process::ExitCode;

const READY_LINE_PREFIX: &str = "READY port=";
use tonic::transport::Server;
use tower_http::cors::{Any, CorsLayer};
use http::header::HeaderName;

const DEFAULT_PORT: u16 = 50052;
const DEFAULT_BIND: &str = "127.0.0.1";

struct LauncherArgs {
    bind: IpAddr,
    port: u16,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS, // --help / --version
        Err(e) => {
            eprintln!("medusa-threed-rs: {e}");
            eprintln!("usage: medusa-threed-rs [--bind ADDR] [--port N]");
            return ExitCode::from(2);
        }
    };

    let addr = SocketAddr::new(args.bind, args.port);
    if let Err(e) = run(addr).await {
        eprintln!("medusa-threed-rs: fatal: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

async fn run(addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any)
        .expose_headers([
            HeaderName::from_static("grpc-status"),
            HeaderName::from_static("grpc-message"),
        ]);

    // Bind first so the ready-line is emitted only after the listen socket
    // is up — the Java launcher waits for it on stderr before connecting.
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    emit_ready(bound);

    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    Server::builder()
        .accept_http1(true)
        .layer(cors)
        .add_service(tonic_web::enable(PointCloudServiceServer::new(PointCloudServiceImpl)))
        .add_service(tonic_web::enable(MeshServiceServer::new(MeshServiceImpl)))
        .add_service(tonic_web::enable(ThreeDScanServiceServer::new(ThreeDScanServiceImpl::new())))
        .serve_with_incoming_shutdown(incoming, shutdown_signal())
        .await?;

    eprintln!("[medusa-threed-rs] shutdown complete");
    Ok(())
}

/// Emit the launcher ready signal.
///
/// Contract:
///   - STDOUT (line-buffered, flushed): exactly `READY port=<N>\n` as the
///     first and only stdout line written by the running server.  The Java
///     launcher blocks reading stdout until it sees this line (10s timeout).
///   - STDERR: a follow-up structured log line with bind details.  All other
///     server output goes on stderr.
fn emit_ready(addr: SocketAddr) {
    {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{}{}", READY_LINE_PREFIX, addr.port());
        let _ = out.flush();
    }
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "[medusa-threed-rs] bind={} port={}", addr.ip(), addr.port());
    let _ = err.flush();
}

async fn shutdown_signal() {
    let stdin_eof = async {
        tokio::task::spawn_blocking(|| {
            let mut buf = [0u8; 256];
            loop {
                match std::io::Read::read(&mut std::io::stdin().lock(), &mut buf) {
                    Ok(0) => return,
                    Ok(_) => continue,
                    Err(_) => return,
                }
            }
        })
        .await
        .ok();
    };

    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let other_signal = async {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut s) = signal(SignalKind::terminate()) {
            s.recv().await;
        } else {
            std::future::pending::<()>().await;
        }
    };

    // Windows console-control: CTRL_BREAK_EVENT and CTRL_CLOSE_EVENT.
    // CTRL_CLOSE_EVENT is what fires for a console process when its window
    // is closed (the WM_CLOSE-equivalent for a console host) or when the
    // parent terminates the console — the closest cross-process "please
    // exit" Windows signal a console child receives without an attached
    // console window.
    #[cfg(windows)]
    let other_signal = async {
        let brk = async {
            if let Ok(mut s) = tokio::signal::windows::ctrl_break() {
                s.recv().await;
            } else {
                std::future::pending::<()>().await;
            }
        };
        let close = async {
            if let Ok(mut s) = tokio::signal::windows::ctrl_close() {
                s.recv().await;
            } else {
                std::future::pending::<()>().await;
            }
        };
        tokio::select! {
            _ = brk => (),
            _ = close => (),
        }
    };

    tokio::select! {
        _ = stdin_eof => eprintln!("[medusa-threed-rs] stdin EOF — shutting down"),
        _ = ctrl_c => eprintln!("[medusa-threed-rs] ctrl-c — shutting down"),
        _ = other_signal => eprintln!("[medusa-threed-rs] term signal — shutting down"),
    }
}

fn parse_args<I: IntoIterator<Item = String>>(argv: I) -> Result<Option<LauncherArgs>, String> {
    let mut bind: IpAddr = DEFAULT_BIND.parse().expect("default bind is valid");
    let mut port: u16 = DEFAULT_PORT;
    let mut it = argv.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                return Ok(None);
            }
            "--version" | "-V" => {
                println!("medusa-threed-rs {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--bind" => {
                let v = it.next().ok_or_else(|| "--bind needs an address".to_string())?;
                bind = v.parse().map_err(|e| format!("invalid --bind '{v}': {e}"))?;
            }
            "--port" => {
                let v = it.next().ok_or_else(|| "--port needs a number".to_string())?;
                port = v.parse().map_err(|e| format!("invalid --port '{v}': {e}"))?;
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(Some(LauncherArgs { bind, port }))
}

fn print_help() {
    println!(
        "medusa-threed-rs {}\n\
         Rust gRPC engine for the medusa-3d module.\n\n\
         USAGE:\n  medusa-threed-rs [--bind ADDR] [--port N]\n\n\
         OPTIONS:\n  \
           --bind ADDR    interface to bind (default {DEFAULT_BIND})\n  \
           --port N       TCP port to listen on (default {DEFAULT_PORT})\n  \
           --help, -h     print this help\n  \
           --version, -V  print version\n\n\
         LAUNCHER CONTRACT:\n  \
           ready-line on stdout (first stdout line): 'READY port=<N>'\n  \
           shutdown: SIGTERM / SIGINT / CTRL_BREAK_EVENT / CTRL_CLOSE_EVENT / stdin EOF\n",
        env!("CARGO_PKG_VERSION")
    );
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn defaults() {
        let a = parse_args(args(&[])).unwrap().unwrap();
        assert_eq!(a.port, DEFAULT_PORT);
        assert_eq!(a.bind.to_string(), DEFAULT_BIND);
    }

    #[test]
    fn explicit_port_and_bind() {
        let a = parse_args(args(&["--port", "60000", "--bind", "0.0.0.0"]))
            .unwrap()
            .unwrap();
        assert_eq!(a.port, 60000);
        assert_eq!(a.bind.to_string(), "0.0.0.0");
    }

    #[test]
    fn unknown_flag_errors() {
        assert!(parse_args(args(&["--frobnicate"])).is_err());
    }

    #[test]
    fn missing_value_errors() {
        assert!(parse_args(args(&["--port"])).is_err());
        assert!(parse_args(args(&["--bind"])).is_err());
    }

    #[test]
    fn invalid_port_errors() {
        assert!(parse_args(args(&["--port", "notanumber"])).is_err());
        assert!(parse_args(args(&["--port", "70000"])).is_err());
    }

    #[test]
    fn invalid_bind_errors() {
        assert!(parse_args(args(&["--bind", "not.an.ip.addr"])).is_err());
    }

    #[test]
    fn help_returns_none() {
        assert!(parse_args(args(&["--help"])).unwrap().is_none());
        assert!(parse_args(args(&["--version"])).unwrap().is_none());
    }

    /// Pin the launcher ready-line prefix so the protocol contract can't
    /// drift accidentally — the Java side reads stdout looking for exactly
    /// this prefix.
    #[test]
    fn ready_line_prefix_is_stable() {
        assert_eq!(super::READY_LINE_PREFIX, "READY port=");
    }
}
