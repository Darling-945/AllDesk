mod registry;
mod relay;
mod signaling;
mod stun;
mod config;
mod metrics;
mod turn;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{error, info, warn};

/// Default signaling server port (WebSocket).
const DEFAULT_SIGNALING_PORT: u16 = 21118;

/// Default relay server port (QUIC).
const DEFAULT_RELAY_PORT: u16 = 21119;

/// Default STUN server port (UDP).
const DEFAULT_STUN_PORT: u16 = 21116;

/// Default TURN server port (UDP).
const DEFAULT_TURN_PORT: u16 = 21117;

/// Default health check port (HTTP).
const DEFAULT_HEALTH_PORT: u16 = 21120;

/// Default Prometheus metrics port (HTTP).
const DEFAULT_METRICS_PORT: u16 = 21121;

/// Default log format.
const DEFAULT_JSON_LOGS: bool = false;

struct ServerArgs {
    signaling_port: u16,
    relay_port: u16,
    stun_port: u16,
    turn_port: u16,
    health_port: u16,
    metrics_port: u16,
    json_logs: bool,
    tls_cert: Option<String>,
    tls_key: Option<String>,
}

fn parse_args() -> ServerArgs {
    let args: Vec<String> = std::env::args().collect();

    let mut signaling_port = DEFAULT_SIGNALING_PORT;
    let mut relay_port = DEFAULT_RELAY_PORT;
    let mut stun_port = DEFAULT_STUN_PORT;
    let mut turn_port = DEFAULT_TURN_PORT;
    let mut health_port = DEFAULT_HEALTH_PORT;
    let mut metrics_port = DEFAULT_METRICS_PORT;
    let mut json_logs = DEFAULT_JSON_LOGS;
    let mut tls_cert: Option<String> = None;
    let mut tls_key: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--signaling-port" | "-s" => {
                if i + 1 < args.len() {
                    signaling_port = args[i + 1].parse().unwrap_or_else(|_| {
                        eprintln!(
                            "Invalid signaling port: {}, using default {}",
                            args[i + 1], DEFAULT_SIGNALING_PORT
                        );
                        DEFAULT_SIGNALING_PORT
                    });
                    i += 1;
                }
            }
            "--relay-port" | "-r" => {
                if i + 1 < args.len() {
                    relay_port = args[i + 1].parse().unwrap_or_else(|_| {
                        eprintln!(
                            "Invalid relay port: {}, using default {}",
                            args[i + 1], DEFAULT_RELAY_PORT
                        );
                        DEFAULT_RELAY_PORT
                    });
                    i += 1;
                }
            }
            "--stun-port" => {
                if i + 1 < args.len() {
                    stun_port = args[i + 1].parse().unwrap_or_else(|_| {
                        eprintln!(
                            "Invalid STUN port: {}, using default {}",
                            args[i + 1], DEFAULT_STUN_PORT
                        );
                        DEFAULT_STUN_PORT
                    });
                    i += 1;
                }
            }
            "--turn-port" => {
                if i + 1 < args.len() {
                    turn_port = args[i + 1].parse().unwrap_or_else(|_| {
                        eprintln!(
                            "Invalid TURN port: {}, using default {}",
                            args[i + 1], DEFAULT_TURN_PORT
                        );
                        DEFAULT_TURN_PORT
                    });
                    i += 1;
                }
            }
            "--health-port" => {
                if i + 1 < args.len() {
                    health_port = args[i + 1].parse().unwrap_or_else(|_| {
                        eprintln!(
                            "Invalid health port: {}, using default {}",
                            args[i + 1], DEFAULT_HEALTH_PORT
                        );
                        DEFAULT_HEALTH_PORT
                    });
                    i += 1;
                }
            }
            "--metrics-port" => {
                if i + 1 < args.len() {
                    metrics_port = args[i + 1].parse().unwrap_or_else(|_| {
                        eprintln!(
                            "Invalid metrics port: {}, using default {}",
                            args[i + 1], DEFAULT_METRICS_PORT
                        );
                        DEFAULT_METRICS_PORT
                    });
                    i += 1;
                }
            }
            "--json-logs" => {
                json_logs = true;
            }
            "--tls-cert" => {
                if i + 1 < args.len() {
                    tls_cert = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--tls-key" => {
                if i + 1 < args.len() {
                    tls_key = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--help" | "-h" => {
                println!("AllDesk Signaling + Relay Server");
                println!();
                println!("Usage: alldesk-server [OPTIONS]");
                println!();
                println!("Options:");
                println!("  -s, --signaling-port <PORT>  WebSocket signaling port (default: {})", DEFAULT_SIGNALING_PORT);
                println!("  -r, --relay-port <PORT>      QUIC relay port (default: {})", DEFAULT_RELAY_PORT);
                println!("      --stun-port <PORT>       STUN UDP port (default: {})", DEFAULT_STUN_PORT);
                println!("      --turn-port <PORT>       TURN UDP port (default: {})", DEFAULT_TURN_PORT);
                println!("      --health-port <PORT>     HTTP health check port (default: {})", DEFAULT_HEALTH_PORT);
                println!("      --metrics-port <PORT>    Prometheus metrics port (default: {})", DEFAULT_METRICS_PORT);
                println!("      --tls-cert <PATH>        TLS certificate PEM file (enables WSS)");
                println!("      --tls-key <PATH>         TLS private key PEM file (enables WSS)");
                println!("      --json-logs              Enable structured JSON logging");
                println!("  -h, --help                   Show this help message");
                std::process::exit(0);
            }
            other => {
                eprintln!("Unknown option: {}", other);
                eprintln!("Use --help for usage information");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    ServerArgs { signaling_port, relay_port, stun_port, turn_port, health_port, metrics_port, json_logs, tls_cert, tls_key }
}

/// Run a simple HTTP health check endpoint on the given port.
async fn run_health_server(port: u16, shutdown: Arc<AtomicBool>) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!("Health check HTTP endpoint listening on port {}", port);

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((mut stream, _addr)) => {
                        if shutdown.load(Ordering::Relaxed) {
                            break;
                        }
                        let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 16\r\n\r\n{\"status\":\"ok\"}";
                        let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
                    }
                    Err(e) => {
                        warn!("Health check accept error: {}", e);
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    }

    info!("Health check server shut down");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = parse_args();

    // Initialize logging
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    if args.json_logs {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .with_target(true)
            .with_thread_ids(false)
            .with_file(false)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(true)
            .with_thread_ids(false)
            .with_file(false)
            .init();
    }

    info!("Starting AllDesk server");
    info!("  Signaling (WebSocket): port {}", args.signaling_port);
    info!("  Relay (QUIC):          port {}", args.relay_port);
    info!("  STUN (UDP):            port {}", args.stun_port);
    info!("  TURN (UDP):            port {}", args.turn_port);
    info!("  Health check (HTTP):   port {}", args.health_port);

    // Initialize Prometheus metrics exporter
    if let Err(e) = metrics::init_metrics(args.metrics_port) {
        warn!("Failed to start Prometheus metrics exporter: {}", e);
    } else {
        info!("  Metrics (Prometheus):  port {}", args.metrics_port);
    }

    // Shared peer registry
    let registry = registry::PeerRegistry::new();

    // Signaling broadcaster for sending messages to connected peers
    let broadcaster = signaling::SignalingBroadcaster::new();

    // Relay server
    let relay_server = relay::RelayServer::new(
        registry.clone(),
        broadcaster.clone(),
        args.relay_port,
    );

    // Shutdown signal
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_signaling = shutdown.clone();
    let shutdown_health = shutdown.clone();
    let shutdown_relay = shutdown.clone();
    let shutdown_stun = shutdown.clone();
    let shutdown_turn = shutdown.clone();

    // Setup graceful shutdown via Ctrl+C
    let ctrlc_shutdown = shutdown.clone();
    ctrlc::set_handler(move || {
        info!("Received shutdown signal (Ctrl+C), draining...");
        ctrlc_shutdown.store(true, Ordering::Relaxed);
    })?;

    // Authentication
    let auth = signaling::ServerAuth::from_env();

    // TLS configuration (optional)
    let tls_config = match (&args.tls_cert, &args.tls_key) {
        (Some(cert), Some(key)) => {
            match signaling::TlsConfig::from_files(cert, key) {
                Ok(tls) => {
                    info!("  TLS enabled for signaling (WSS)");
                    Some(tls)
                }
                Err(e) => {
                    warn!("Failed to load TLS config, falling back to WS: {}", e);
                    None
                }
            }
        }
        _ => None,
    };

    // Start all servers concurrently
    let signaling_registry = registry.clone();
    let signaling_broadcaster = broadcaster.clone();
    let signaling_relay = relay_server.clone();
    let signaling_port = args.signaling_port;
    let stun_port = args.stun_port;
    let turn_port = args.turn_port;
    let health_port = args.health_port;
    let relay_port = args.relay_port;

    tokio::select! {
        result = tokio::spawn(async move {
            signaling::run_signaling_server(
                signaling_port,
                signaling_registry,
                signaling_broadcaster,
                signaling_relay,
                shutdown_signaling,
                auth,
                tls_config,
            )
            .await
        }) => {
            match result {
                Ok(Ok(())) => info!("Signaling server shut down cleanly"),
                Ok(Err(e)) => error!("Signaling server error: {}", e),
                Err(e) => error!("Signaling server task panicked: {}", e),
            }
        }
        result = tokio::spawn(stun::run_stun_server(stun_port, shutdown_stun)) => {
            match result {
                Ok(Ok(())) => info!("STUN server shut down cleanly"),
                Ok(Err(e)) => error!("STUN server error: {}", e),
                Err(e) => error!("STUN server task panicked: {}", e),
            }
        }
        result = tokio::spawn(turn::run_turn_server(turn_port, shutdown_turn)) => {
            match result {
                Ok(Ok(())) => info!("TURN server shut down cleanly"),
                Ok(Err(e)) => error!("TURN server error: {}", e),
                Err(e) => error!("TURN server task panicked: {}", e),
            }
        }
        result = tokio::spawn(async move {
            relay_server.start(relay_port).await
        }) => {
            match result {
                Ok(Ok(())) => info!("Relay server shut down cleanly"),
                Ok(Err(e)) => error!("Relay server error: {}", e),
                Err(e) => error!("Relay server task panicked: {}", e),
            }
        }
        result = tokio::spawn(run_health_server(health_port, shutdown_health)) => {
            match result {
                Ok(Ok(())) => info!("Health check server shut down cleanly"),
                Ok(Err(e)) => error!("Health check server error: {}", e),
                Err(e) => error!("Health check server task panicked: {}", e),
            }
        }
        _ = tokio::spawn(async move {
            // Wait for shutdown signal
            while !shutdown_relay.load(Ordering::Relaxed) {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }) => {
            info!("Shutdown signal received");
        }
    }

    info!("AllDesk server shutting down");
    Ok(())
}
