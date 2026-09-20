# 🌍 Azeroth Dashboard (Rust Edition)

Welcome to **Azeroth Dashboard**, a high-performance administration and telemetry dashboard designed for managing AzerothCore instances (and similar MMORPG server architectures). Built entirely in Rust from the ground up, it leverages the `ferrox` backend ecosystem and the `ferrox-front` WebAssembly framework to deliver unprecedented performance, type safety, and real-time observability.

## 1. What It Is & Architectural Purpose
Azeroth Dashboard serves as the unified command center for game administrators, developers, and operators. Its architectural purpose is to replace disjointed, heavy administrative web panels (often written in PHP, Node.js, or React) with a single, highly concurrent binary and a lightning-fast Wasm frontend. It securely monitors server metrics, manages players/accounts, and handles real-time configuration without the overhead of garbage-collected languages or Virtual DOM bottlenecks.

## 2. Architectural Layering
The project is strictly divided into two primary execution layers that share data models:

| Layer | Technology | Purpose |
| :--- | :--- | :--- |
| **Backend API** | `axum`, `tokio`, `ferrox-*` | Highly concurrent async Rust server handling REST endpoints, DB connections, and system metric extraction (`sysinfo`). |
| **Frontend Wasm** | `ferrox-front-*` | Rust-compiled WebAssembly frontend providing 60 FPS rendering, using fine-grained reactive signals and `ferrox-front-ui` components. |
| **Shared Models** | Rust Structs | End-to-end type safety between the server response and the client parsing, eliminating API mismatch errors. |

## 3. How It Works Under the Hood
The backend (`src/main.rs`) initializes an `axum` router equipped with `tower-http` CORS and telemetry layers. It exposes specific REST endpoints (like `/api/stats` and `/api/server-status`) which gather real-time data from the host operating system.
On the client side (`frontend/src/main.rs`), the `ferrox-front` WebAssembly application uses reactive signals to query these endpoints at regular intervals. When a network response is received, the data is deserialized directly into Rust structs and the corresponding reactive signals are updated. The `ferrox-front-core` engine instantly applies these updates to specific DOM nodes, completely bypassing virtual DOM diffing.

## 4. Why It Was Designed This Way
Managing high-load game servers requires minimal administrative overhead. Traditional web dashboards can introduce significant memory and CPU bloat. This Rust-native architecture was chosen because:
1. **Zero Garbage Collection**: The frontend runs at native speed in the browser, providing a buttery smooth UI for telemetry visualization.
2. **Minimal Resource Footprint**: The backend server requires only a few megabytes of RAM, meaning it can run natively alongside the game server without stealing resources from the main `worldserver` process.
3. **End-to-End Type Safety**: By writing both the frontend and backend in Rust, serialization mismatches and "undefined is not a function" errors are eliminated at compile time.

## 5. Practical Usage Guide & Extended Code Examples

### Prerequisites
- [Rust](https://rustup.rs/) (latest stable)
- [Trunk](https://trunkrs.dev/) for compiling the Wasm frontend

### Running the Project Locally

1. **Start the Backend API Server**:
```bash
cargo run
```
*The API will start on `http://127.0.0.1:3000`.*

2. **Start the Frontend Wasm Dev Server**:
```bash
cd frontend
trunk serve --port 8085
```
*The UI will be accessible at `http://localhost:8085`.*

### Code Example: Fetching Real-time Stats
```rust
// In frontend/src/main.rs
use ferrox_front_core::prelude::*;
use reqwest;

#[component]
pub fn ServerStats() -> Element {
    let (cpu_load, set_cpu_load) = create_signal(0.0);
    
    // Fetch telemetry data from the backend
    spawn_local(async move {
        if let Ok(res) = reqwest::get("http://127.0.0.1:3000/api/stats").await {
            if let Ok(data) = res.json::<ServerTelemetry>().await {
                set_cpu_load.set(data.cpu_usage);
            }
        }
    });

    rsx! {
        div {
            "Current CPU Load: " { move || format!("{:.1}%", cpu_load.get()) }
        }
    }
}
```

## 6. Anti-Patterns: How NOT to Use It
> [!CAUTION]
> **Anti-Pattern: Mixing Heavy JS Libraries**
> Do not attempt to embed heavy JavaScript visualization libraries (like Highcharts or D3.js) into this dashboard. Since the architecture relies on `ferrox-front`, adding JS dependencies will introduce context-switching latency between Wasm and the JS engine. Use `ferrox-front-charts` instead for native WebGL rendering.

## 7. Pro-Tips & Best Practices
> [!TIP]
> **Pro-Tip: Shared Data Contracts**
> Currently, the backend and frontend duplicate some structural definitions (like the JSON responses). For a production deployment, extract these definitions into a shared `azeroth-dashboard-types` crate and import it in both `Cargo.toml` files. This guarantees 100% API contract compliance!
