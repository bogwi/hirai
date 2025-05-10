// src/main.rs

//! # Hirai Main Entry Point
//!
//! This is the main entry point for the Hirai application. It initializes configuration, logging, and launches the core async tasks for file watching, broadcasting, listening, and the optional web UI.
//!
//! ## Modules
//!
//! - [`config`]: Handles configuration loading and merging from CLI, file, and environment.
//! - [`event`]: Defines the file event struct and serialization.
//! - [`watcher`]: Implements the file system watcher for change detection.
//! - [`network`]: Contains UDP multicast broadcaster and listener logic.
//! - [`web`]: Provides the web server and WebSocket event delivery.

mod config;
mod event;
mod network;
mod watcher;
mod web;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use crate::config::AppConfig;
use crate::event::Event;

/// The main entry point for the Hirai application.
///
/// This function performs the following steps:
/// 1. Loads the application configuration from CLI, file, and environment.
/// 2. Initializes the tracing subscriber for logging.
/// 3. Sets up communication channels for event propagation between components.
/// 4. Spawns async tasks for the watcher, broadcaster, listener, and web server as needed.
/// 5. Forwards events between watcher, broadcaster, and web server.
/// 6. Waits for a Ctrl-C signal to initiate graceful shutdown of all tasks.
///
/// # Returns
/// Returns `Ok(())` if the application exits cleanly, or an error if initialization fails.
#[tokio::main]
async fn main() -> Result<()> {
    let app_config = match AppConfig::load() {
        Ok(cfg) => Arc::new(cfg),
        Err(e) => {
            eprintln!("Error loading configuration: {}", e);
            // Use clap to print help if args are involved in error
            // use clap::CommandFactory;
            // config::CliArgs::command().print_help()?;
            std::process::exit(1);
        }
    };

    // Initialize tracing subscriber for logging with environment filter and max level.
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&app_config.log_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(filter)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(std::io::stderr) // Log to stderr
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("Setting default tracing subscriber failed");

    tracing::info!("Hirai starting with configuration: {:?}", app_config);

    // Shutdown signal channel for graceful shutdown of all tasks.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Channel for events from watcher to main logic.
    let (watcher_event_tx, mut watcher_event_rx) = mpsc::channel::<Event>(100);

    // Channel for events from main logic to broadcaster.
    let (main_to_broadcaster_tx, main_to_broadcaster_rx) = mpsc::channel::<Event>(100);

    // Channel for events from main logic (or listener) to web server source.
    let (main_to_web_source_tx, main_to_web_source_rx) = mpsc::channel::<Event>(100);

    // Broadcast channel for web server to distribute events to WebSocket clients.
    let (web_event_broadcast_tx, _) = tokio::sync::broadcast::channel::<Event>(100);

    let mut tasks = Vec::new();

    // Launch watcher and broadcaster tasks if not in listener mode.
    if !app_config.listen {
        if app_config.folders_to_watch.is_empty() {
            tracing::error!("BROADCASTER MODE: No folders to watch. Please specify folders via CLI or in hirai.toml.");
            // use clap::CommandFactory;
            // config::CliArgs::command().print_help()?;
            std::process::exit(1);
        }
        tracing::info!(
            "BROADCASTER MODE: Starting watcher for folders: {:?}",
            app_config.folders_to_watch
        );
        let watcher_config = Arc::clone(&app_config);
        let watcher_event_tx_clone = watcher_event_tx.clone(); // For watcher to send events
        tasks.push(tokio::spawn(async move {
            if let Err(e) = watcher::run_watcher(watcher_config, watcher_event_tx_clone).await {
                tracing::error!("Watcher exited with error: {}", e);
            }
        }));

        let broadcaster_config = Arc::clone(&app_config);
        let broadcaster_shutdown_rx = shutdown_rx.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(e) = network::run_broadcaster(
                broadcaster_config,
                main_to_broadcaster_rx,
                broadcaster_shutdown_rx,
            )
            .await
            {
                tracing::error!("Broadcaster exited with error: {}", e);
            }
        }));
    } else {
        tracing::info!("LISTENER MODE: Starting listener.");
        let listener_config = Arc::clone(&app_config);
        let listener_event_tx_for_web = if app_config.browse {
            Some(main_to_web_source_tx.clone())
        } else {
            None
        };
        let listener_shutdown_rx = shutdown_rx.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(e) = network::run_listener(
                listener_config,
                listener_event_tx_for_web,
                listener_shutdown_rx,
            )
            .await
            {
                tracing::error!("Listener exited with error: {}", e);
            }
        }));
    }

    // Launch web server task if browse mode is enabled.
    if app_config.browse {
        tracing::info!(
            "WEB UI enabled: Starting web server on {}",
            app_config.web_addr
        );
        let web_config = Arc::clone(&app_config);
        let web_broadcast_tx_clone = web_event_broadcast_tx.clone();
        let web_shutdown_rx = shutdown_rx.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(e) = web::start_server(
                web_config,
                main_to_web_source_rx,
                web_broadcast_tx_clone,
                web_shutdown_rx,
            )
            .await
            {
                tracing::error!("Web server exited with error: {}", e);
            }
        }));
    }

    // Event forwarding logic (from watcher to broadcaster and/or web)
    let main_loop_shutdown_rx = shutdown_rx.clone();
    let main_loop_task = tokio::spawn(async move {
        let mut shutdown = main_loop_shutdown_rx.clone();
        loop {
            tokio::select! {
                Some(event) = watcher_event_rx.recv() => {
                    tracing::debug!("Main loop received event from watcher: {:?}", event);
                    if !app_config.listen { // If in broadcaster mode, send to broadcaster
                        if let Err(e) = main_to_broadcaster_tx.send(event.clone()).await {
                            tracing::error!("Failed to send event to broadcaster channel: {}", e);
                        }
                    }
                    if app_config.browse { // If browse mode is on, send to web source channel
                        // In listener mode, the listener sends directly to main_to_web_source_tx
                        // In broadcaster mode, watcher events go here.
                        if let Err(e) = main_to_web_source_tx.send(event.clone()).await {
                             tracing::debug!("Failed to send event to web source channel (no web server?): {}", e);
                        }
                    }
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        tracing::info!("Main event loop shutting down.");
                        break;
                    }
                }
                else => {
                    tracing::info!("Watcher event channel closed. Main loop exiting.");
                    break;
                }
            }
        }
    });
    tasks.push(main_loop_task);

    // Wait for Ctrl-C signal to initiate shutdown.
    match tokio::signal::ctrl_c().await {
        Ok(()) => {
            tracing::info!("Ctrl-C received, initiating shutdown...");
        }
        Err(err) => {
            tracing::error!("Failed to listen for Ctrl-C signal: {}", err);
        }
    }

    // Signal all tasks to shutdown.
    if shutdown_tx.send(true).is_err() {
        tracing::error!("Failed to send shutdown signal");
    }

    // Wait for all tasks to complete.
    for task in tasks {
        if let Err(e) = task.await {
            tracing::error!("A task panicked or exited with error: {}", e);
        }
    }

    tracing::info!("Hirai shut down gracefully.");
    Ok(())
}
