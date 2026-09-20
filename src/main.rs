use axum::{
    routing::{get, post},
    Router,
    Json,
    extract::State,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tracing::info;
use tower_http::cors::CorsLayer;
use sysinfo::System;
use rand::Rng;
use std::time::{Instant, Duration};
use lettre::{Message, SmtpTransport, Transport, transport::smtp::authentication::Credentials};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    pub title: String,
    pub description: Option<String>,
    pub panels: Vec<PanelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PanelConfig {
    #[serde(rename = "line_chart")]
    LineChart {
        title: String,
        data: Vec<PointData>,
        width: Option<u32>,
        height: Option<u32>,
        alert: Option<bool>,
    },
    #[serde(rename = "bar_chart")]
    BarChart {
        title: String,
        data: Vec<BarData>,
        width: Option<u32>,
        height: Option<u32>,
        alert: Option<bool>,
    },
    #[serde(rename = "stat_badge")]
    StatBadge {
        title: String,
        value: String,
        color: Option<String>,
        alert: Option<bool>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointData {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarData {
    pub label: String,
    pub value: f64,
    pub color: String,
}

#[derive(Debug, Clone)]
struct MockData {
    online_players: f64,
    total_packets: f64,
    char_queue: f64,
    world_queue: f64,
    login_queue: f64,
    memory_bytes: f64,
    ping_avg: f64,
    net_in: f64,
    net_out: f64,
    scripts_time: f64,
    maps_time: f64,
    sess_time: f64,
    world_update_history: Vec<PointData>,
    time_tick: f64,
}

impl MockData {
    fn new() -> Self {
        Self {
            online_players: 1420.0,
            total_packets: 3200000.0,
            char_queue: 45.0,
            world_queue: 12.0,
            login_queue: 2.0,
            memory_bytes: 268435456.0,
            ping_avg: 45.0,
            net_in: 15728640.0,
            net_out: 131072000.0,
            scripts_time: 0.150,
            maps_time: 0.050,
            sess_time: 0.020,
            time_tick: 5.0,
            world_update_history: vec![
                PointData { x: 1.0, y: 0.035 },
                PointData { x: 2.0, y: 0.042 },
                PointData { x: 3.0, y: 0.031 },
                PointData { x: 4.0, y: 0.089 },
                PointData { x: 5.0, y: 0.036 },
            ],
        }
    }

    fn tick(&mut self) {
        let mut rng = rand::thread_rng();

        macro_rules! vary {
            ($val:expr, $variance:expr, $min:expr, $max:expr) => {
                let chance: f64 = rng.gen();
                if chance > 0.75 {
                    if rng.gen::<bool>() {
                        $val = ($val + $variance).min($max);
                    } else {
                        $val = ($val - $variance).max($min);
                    }
                }
            };
        }

        vary!(self.online_players, 10.0, 0.0, 5000.0);
        self.total_packets += rng.gen_range(500.0..2000.0);
        self.net_in += rng.gen_range(1024.0..50000.0);
        self.net_out += rng.gen_range(50000.0..500000.0);
        vary!(self.char_queue, 1.0, 0.0, 100.0);
        vary!(self.world_queue, 1.0, 0.0, 100.0);
        vary!(self.login_queue, 1.0, 0.0, 10.0);
        vary!(self.memory_bytes, 1048576.0, 100000000.0, 1000000000.0);
        vary!(self.ping_avg, 2.0, 10.0, 500.0);
        vary!(self.scripts_time, 0.005, 0.01, 1.0);
        vary!(self.maps_time, 0.005, 0.01, 1.0);
        vary!(self.sess_time, 0.002, 0.001, 0.1);

        self.time_tick += 1.0;
        let last_y = self.world_update_history.last().unwrap().y;
        let mut new_y = last_y;
        
        // Volatile mock data for World Update Duration so it looks interesting
        if rng.gen::<bool>() {
            new_y = (new_y + rng.gen_range(0.01..0.05)).min(0.2);
        } else {
            new_y = (new_y - rng.gen_range(0.01..0.05)).max(0.01);
        }
        
        // Trigger alerts randomly for testing
        if rng.gen_bool(0.05) {
            self.ping_avg = rng.gen_range(160.0..300.0);
        }
        if rng.gen_bool(0.05) {
            self.char_queue = rng.gen_range(55.0..100.0);
        }
        
        self.world_update_history.push(PointData { x: self.time_tick, y: new_y });
        if self.world_update_history.len() > 10 {
            self.world_update_history.remove(0);
        }
    }
}

type AppState = Arc<Mutex<(MockData, AlertManager)>>;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    
    let cors = CorsLayer::permissive();
    let state = Arc::new(Mutex::new((MockData::new(), AlertManager::new())));
    
    let app = Router::new()
        .route("/api/dashboard/config", get(get_dashboard_config))
        .route("/api/dashboard/layouts", post(save_layout))
        .route("/api/dashboard/layouts", get(get_layout))
        .with_state(state)
        .layer(cors);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8086));
    tracing::info!("AzerothDashboard listening on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn get_dashboard_config(State(state): State<AppState>) -> Json<DashboardConfig> {
    let client = reqwest::Client::new();
    
    // Fetch metrics with a short timeout so we don't block forever if AC is offline
    let res = client
        .get("http://127.0.0.1:9200/metrics")
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await;

    let (metrics_text, is_live) = match res {
        Ok(response) if response.status().is_success() => {
            (response.text().await.unwrap_or_default(), true)
        }
        _ => ("".to_string(), false), // Fallback to mock
    };

    let title = if is_live {
        "AzerothCore Observability (LIVE)".to_string()
    } else {
        "AzerothCore Observability (MOCK - Server Offline)".to_string()
    };

    let (
        online_players, total_packets, char_queue, world_queue, login_queue,
        memory_bytes, ping_avg, net_in, net_out, scripts_time, maps_time, sess_time, world_history
    ) = if is_live {
        let metrics = parse_prometheus_metrics(&metrics_text);
        
        let mut sys = System::new_all();
        sys.refresh_all();
        let mut mem_usage_bytes = 0;
        for (_pid, process) in sys.processes() {
            if process.name().to_string_lossy().to_lowercase().contains("worldserver") {
                mem_usage_bytes += process.memory();
            }
        }
        
        let mem = if mem_usage_bytes > 0 { mem_usage_bytes as f64 } else { *metrics.get("ac_world_memory_rss_bytes").unwrap_or(&268435456.0) };
        let psum = metrics.get("ac_world_player_ping_milliseconds_sum").unwrap_or(&0.0);
        let pcount = metrics.get("ac_world_player_ping_milliseconds_count").unwrap_or(&0.0);
        let pavg = if *pcount > 0.0 { psum / pcount } else { 0.0 };

        (
            *metrics.get("ac_world_online_players").unwrap_or(&0.0),
            *metrics.get("ac_world_processed_packets_total").unwrap_or(&0.0),
            *metrics.get("ac_world_character_database_queue").unwrap_or(&0.0),
            *metrics.get("ac_world_world_database_queue").unwrap_or(&0.0),
            *metrics.get("ac_world_login_database_queue").unwrap_or(&0.0),
            mem,
            pavg,
            *metrics.get("ac_world_network_bytes_received_total").unwrap_or(&0.0),
            *metrics.get("ac_world_network_bytes_sent_total").unwrap_or(&0.0),
            *metrics.get("ac_world_update_phase_duration_seconds_sum{phase=\"update_world_scripts\"}").unwrap_or(&0.0),
            *metrics.get("ac_world_update_phase_duration_seconds_sum{phase=\"update_maps\"}").unwrap_or(&0.0),
            *metrics.get("ac_world_update_phase_duration_seconds_sum{phase=\"update_sessions\"}").unwrap_or(&0.0),
            vec![
                PointData { x: 1.0, y: *metrics.get("ac_world_update_duration_seconds_sum").unwrap_or(&0.0) }
            ]
        )
    } else {
        let mut mock = state.lock().unwrap();
        mock.0.tick();
        (
            mock.0.online_players, mock.0.total_packets, mock.0.char_queue, mock.0.world_queue, mock.0.login_queue,
            mock.0.memory_bytes, mock.0.ping_avg, mock.0.net_in, mock.0.net_out, mock.0.scripts_time, mock.0.maps_time, mock.0.sess_time,
            mock.0.world_update_history.clone()
        )
    };

    let packets_str = if total_packets >= 1_000_000.0 {
        format!("{:.1}M", total_packets / 1_000_000.0)
    } else if total_packets >= 1_000.0 {
        format!("{:.1}k", total_packets / 1_000.0)
    } else {
        total_packets.to_string()
    };

    let memory_str = format!("{:.1} MB", memory_bytes / (1024.0 * 1024.0));
    let ping_str = format!("{:.0} ms", ping_avg);
    
    // CPU usage is dynamic regardless of mock state (if worldserver is running locally)
    let mut sys = System::new_all();
    sys.refresh_all();
    let mut cpu_usage = 0.0;
    let mut actual_mem_usage_bytes = 0;
    let total_memory = sys.total_memory();
    for (_pid, process) in sys.processes() {
        if process.name().to_string_lossy().to_lowercase().contains("worldserver") {
            cpu_usage += process.cpu_usage();
            actual_mem_usage_bytes += process.memory();
        }
    }
    // In mock mode, fake CPU if no worldserver is found
    if !is_live && cpu_usage == 0.0 {
        cpu_usage = rand::thread_rng().gen_range(5.0..25.0);
        
        // Randomly spike CPU for alert testing
        if rand::thread_rng().gen_bool(0.05) {
            cpu_usage = rand::thread_rng().gen_range(86.0..99.0);
        }
    }
    
    let cpu_str = if cpu_usage > 0.0 { format!("{:.1}%", cpu_usage) } else { "0.0%".to_string() };
    let cpu_alert = cpu_usage > 85.0;

    let mem_usage_ratio = if is_live && total_memory > 0 { actual_mem_usage_bytes as f64 / total_memory as f64 } else { memory_bytes / 16_000_000_000.0 };
    let mem_percent = format!("{:.1}%", mem_usage_ratio * 100.0);
    let mem_alert = mem_usage_ratio > 0.90;

    let config = DashboardConfig {
        title,
        description: Some("Disaster-Ready Metrics (System & Backend)".to_string()),
        panels: vec![
            PanelConfig::StatBadge {
                title: "Server CPU Usage (%)".to_string(),
                value: cpu_str,
                color: Some("#f43f5e".to_string()),
                alert: Some(cpu_alert),
            },
            PanelConfig::StatBadge {
                title: "Server RAM Usage (%)".to_string(),
                value: mem_percent,
                color: Some("#10b981".to_string()),
                alert: Some(mem_alert),
            },
            PanelConfig::StatBadge {
                title: "Online Players (ac_world_online_players)".to_string(),
                value: format!("{:.0}", online_players),
                color: Some("#06b6d4".to_string()),
                alert: Some(false),
            },
            PanelConfig::StatBadge {
                title: "Total Packets (ac_world_packets_total)".to_string(),
                value: packets_str,
                color: Some("#8b5cf6".to_string()),
                alert: Some(false),
            },
            PanelConfig::LineChart {
                title: "World Update Duration (s)".to_string(),
                data: world_history.clone(),
                width: Some(400),
                height: Some(250),
                alert: Some(world_history.last().map(|p| p.y > 0.15).unwrap_or(false)),
            },
            PanelConfig::StatBadge {
                title: "Memory (RSS)".to_string(),
                value: memory_str,
                color: Some("#ec4899".to_string()),
                alert: Some(memory_bytes > 1_500_000_000.0),
            },
            PanelConfig::StatBadge {
                title: "Average Ping".to_string(),
                value: ping_str,
                color: Some("#2dd4bf".to_string()),
                alert: Some(ping_avg > 150.0),
            },
            PanelConfig::BarChart {
                title: "Network Traffic (MB)".to_string(),
                data: vec![
                    BarData { label: "IN".to_string(), value: net_in / (1024.0 * 1024.0), color: "#3b82f6".to_string() },
                    BarData { label: "OUT".to_string(), value: net_out / (1024.0 * 1024.0), color: "#f43f5e".to_string() },
                ],
                width: Some(400),
                height: Some(250),
                alert: Some(false),
            },
            PanelConfig::BarChart {
                title: "Update Phases (s)".to_string(),
                data: vec![
                    BarData { label: "Scripts".to_string(), value: scripts_time, color: "#eab308".to_string() },
                    BarData { label: "Maps".to_string(), value: maps_time, color: "#10b981".to_string() },
                    BarData { label: "Sessions".to_string(), value: sess_time, color: "#8b5cf6".to_string() },
                ],
                width: Some(400),
                height: Some(250),
                alert: Some(scripts_time + maps_time + sess_time > 0.2),
            },
            PanelConfig::BarChart {
                title: "Database Queue Size".to_string(),
                data: vec![
                    BarData { label: "Character".to_string(), value: char_queue, color: "#3b82f6".to_string() },
                    BarData { label: "World".to_string(), value: world_queue, color: "#10b981".to_string() },
                    BarData { label: "Login".to_string(), value: login_queue, color: "#f59e0b".to_string() },
                ],
                width: Some(400),
                height: Some(250),
                alert: Some(char_queue > 50.0 || world_queue > 50.0 || login_queue > 50.0),
            },
        ]
    };
    
    let any_alert = cpu_alert || mem_alert || ping_avg > 150.0 || (memory_bytes > 1_500_000_000.0) || (scripts_time + maps_time + sess_time > 0.2) || (char_queue > 50.0 || world_queue > 50.0 || login_queue > 50.0) || (world_history.last().map(|p| p.y > 0.15).unwrap_or(false));

    if any_alert {
        let mut st = state.lock().unwrap();
        if st.1.should_alert() {
            tracing::info!("ALERT THRESHOLD MET. Dispatching notifications...");
            tokio::spawn(async move {
                dispatch_alerts().await;
            });
        }
    }
    
    Json(config)
}

fn parse_prometheus_metrics(text: &str) -> std::collections::HashMap<String, f64> {
    let mut metrics = std::collections::HashMap::new();
    for line in text.lines() {
        if line.starts_with('#') || line.is_empty() { continue; }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let name_part = parts[0];
            let name = if let Some(idx) = name_part.find('{') {
                let raw_name = &name_part[..idx];
                if let Ok(value) = parts[1].parse::<f64>() {
                    metrics.insert(raw_name.to_string(), value);
                }
                name_part
            } else {
                name_part
            };
            
            if let Ok(value) = parts[1].parse::<f64>() {
                metrics.insert(name.to_string(), value);
            }
        }
    }
    metrics
}

async fn save_layout() -> axum::http::StatusCode {
    // Stub for now. Frontend will use LocalStorage.
    axum::http::StatusCode::NOT_IMPLEMENTED
}

async fn get_layout() -> axum::http::StatusCode {
    // Stub for now. Frontend will use LocalStorage.
    axum::http::StatusCode::NOT_IMPLEMENTED
}

#[derive(Debug)]
struct AlertManager {
    last_alert_time: Option<Instant>,
    cooldown: Duration,
}

impl AlertManager {
    fn new() -> Self {
        Self {
            last_alert_time: None,
            cooldown: Duration::from_secs(300), // 5 minutes cooldown
        }
    }

    fn should_alert(&mut self) -> bool {
        if let Some(last) = self.last_alert_time {
            if last.elapsed() < self.cooldown {
                return false;
            }
        }
        self.last_alert_time = Some(Instant::now());
        true
    }
}

async fn dispatch_alerts() {
    let env = std::env::var("ENV").unwrap_or_else(|_| "dev".to_string());
    
    // Discord alert
    let discord_url = std::env::var("DISCORD_BRIDGE_URL").unwrap_or_else(|_| "http://localhost:8080/api/v1/announce".to_string());
    let discord_secret = std::env::var("DISCORD_BRIDGE_SECRET").unwrap_or_else(|_| "super_secret_key_123".to_string());
    
    let client = reqwest::Client::new();
    let res = client.post(&discord_url)
        .header("Authorization", format!("Bearer {}", discord_secret))
        .json(&serde_json::json!({
            "content": "🚨 **AzerothCore Alert!** 🚨\nOne or more metrics have exceeded the safe threshold. Please check the dashboard immediately.",
            "channel": std::env::var("DISCORD_CHANNEL").unwrap_or_else(|_| "alerts".to_string())
        }))
        .send()
        .await;
    
    if let Err(e) = res {
        tracing::error!("Failed to send Discord alert: {}", e);
    }
    
    // Email alert (only if not dev)
    if env != "dev" {
        let email_to = std::env::var("ALERT_EMAIL_TO").unwrap_or_default();
        let smtp_host = std::env::var("SMTP_HOST").unwrap_or_default();
        let smtp_user = std::env::var("SMTP_USER").unwrap_or_default();
        let smtp_pass = std::env::var("SMTP_PASS").unwrap_or_default();
        
        if !email_to.is_empty() && !smtp_host.is_empty() {
            if let (Ok(from), Ok(to)) = (format!("alerts@{}", smtp_host).parse(), email_to.parse()) {
                let email = Message::builder()
                    .from(from)
                    .to(to)
                    .subject("AzerothCore Dashboard Alert")
                    .body(String::from("Alert thresholds exceeded on the dashboard! Please check immediately."))
                    .unwrap();
                    
                let creds = Credentials::new(smtp_user, smtp_pass);
                let mailer = SmtpTransport::relay(&smtp_host)
                    .unwrap()
                    .credentials(creds)
                    .build();
                    
                if let Err(e) = mailer.send(&email) {
                    tracing::error!("Failed to send Email alert: {}", e);
                }
            } else {
                tracing::error!("Failed to parse email addresses for alert.");
            }
        }
    }
}
