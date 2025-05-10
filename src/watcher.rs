// src/watcher.rs
use crate::config::AppConfig;
use crate::event::Event;
use anyhow::Result;
use notify::Watcher as NotifyWatcherTrait; // To use .watcher() and .cache()
use notify_debouncer_full::{new_debouncer, DebouncedEvent};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use tracing::{debug, error, info, warn};

/// Runs the file system watcher in a background thread.
///
/// This function spawns a new thread that sets up a debounced file system watcher
/// for the folders specified in the provided `AppConfig`. File system events are
/// debounced and sent as `Event` objects through the provided Tokio mpsc channel.
///
/// # Arguments
///
/// * `app_config` - Shared application configuration containing folders to watch.
/// * `event_tx` - Tokio mpsc sender for sending debounced file events to the async context.
///
/// # Returns
///
/// Returns `Ok(())` immediately after spawning the watcher thread. The watcher thread
/// continues running in the background, sending events as they occur.
///
/// # Errors
///
/// Returns an error only if the thread cannot be spawned (which is unlikely).
pub async fn run_watcher(app_config: Arc<AppConfig>, event_tx: Sender<Event>) -> Result<()> {
    // This function now only spawns the watcher thread and returns immediately.
    // The actual watcher logic, including debouncer creation, is inside the thread.
    std::thread::spawn(move || {
        // app_config and event_tx are moved into the thread
        let thread_app_config = app_config; // Rename for clarity within thread
        let folders_to_watch = thread_app_config.folders_to_watch.clone();

        // This is the std::sync::mpsc channel for communication between the debouncer and this thread.
        let (debouncer_internal_tx, debouncer_internal_rx) = std::sync::mpsc::channel();

        // Create debouncer. It will live as long as this thread.
        let mut debouncer = match new_debouncer(
            Duration::from_secs(1), // 1-second debounce for responsiveness
            None,                   // No tick channel needed for simple debouncing
            debouncer_internal_tx,  // The debouncer will send its events to this channel
        ) {
            Ok(d) => d,
            Err(e) => {
                error!("[WatcherThread] Failed to create debouncer: {}", e);
                return; // Exit thread if debouncer can't be created
            }
        };

        // Setup watches for each folder
        for folder_str in &folders_to_watch {
            let path = Path::new(folder_str);
            if !path.exists() {
                warn!(
                    "[WatcherThread] Path does not exist, skipping: {}",
                    folder_str
                );
                continue;
            }
            if !path.is_dir() {
                warn!(
                    "[WatcherThread] Path is not a directory, skipping: {}",
                    folder_str
                );
                continue;
            }
            // Use the Watcher trait method from notify::Watcher
            match debouncer
                .watcher() // Gets a mutable reference to the actual notify::Watcher
                .watch(path, notify::RecursiveMode::Recursive)
            {
                Ok(_) => info!("[WatcherThread] Watching folder: {}", folder_str),
                Err(e) => error!(
                    "[WatcherThread] Failed to watch folder {}: {}",
                    folder_str, e
                ),
            }
            // Add the root path to the debouncer's cache for proper event handling
            debouncer
                .cache()
                .add_root(path, notify::RecursiveMode::Recursive);
        }

        info!(
            "[WatcherThread] File system watcher thread started for {:?}",
            folders_to_watch
        );

        // Event processing loop for this thread
        loop {
            match debouncer_internal_rx.recv() {
                // Receive events from the debouncer
                Ok(debouncer_result) => {
                    match debouncer_result {
                        Ok(events) => {
                            for debounced_event in events {
                                // Pass the tokio mpsc sender (event_tx) to handle_debounced_event
                                handle_debounced_event(&debounced_event, &event_tx);
                            }
                        }
                        Err(errors) => {
                            // These are errors from the debouncer itself (e.g., failed to process an event)
                            for error in errors {
                                error!("[WatcherThread] Debouncer reported error: {:?}", error);
                            }
                        }
                    }
                }
                Err(e) => {
                    // This is std::sync::mpsc::RecvError
                    // This error means the sender (debouncer_internal_tx, held by the debouncer)
                    // has been dropped or disconnected. This typically happens when the debouncer is dropped.
                    error!("[WatcherThread] Debouncer internal channel error: {:?}. Watcher thread exiting.", e);
                    break; // Exit loop, and thus the thread
                }
            }
        }
        info!("[WatcherThread] Exiting normally.");
        // debouncer is dropped here when the thread scope ends, cleaning up watches.
    });

    Ok(()) // The main async function returns, thread continues in background.
}

/// Handles a single debounced file system event and sends it to the async event channel.
///
/// This function converts a `DebouncedEvent` into an `Event` struct and sends it
/// through the provided Tokio mpsc sender. If the event kind is not recognized,
/// it is ignored.
///
/// # Arguments
///
/// * `debounced_event` - The debounced file system event to handle.
/// * `event_tx` - Tokio mpsc sender for sending the event to the async context.
fn handle_debounced_event(debounced_event: &DebouncedEvent, event_tx: &Sender<Event>) {
    if debounced_event.paths.is_empty() {
        debug!(
            "Received debounced event with no paths: {:?}",
            debounced_event
        );
        return;
    }

    // For simplicity, take the first path. notify events can have multiple paths (e.g. rename).
    let path_str = debounced_event.paths[0].to_string_lossy().to_string();

    let op_str = match debounced_event.kind {
        notify::event::EventKind::Create(_) => "CREATE",
        notify::event::EventKind::Modify(notify::event::ModifyKind::Data(_)) => "WRITE",
        notify::event::EventKind::Modify(notify::event::ModifyKind::Name(
            notify::event::RenameMode::To,
        )) => "CREATE", // Renamed to new file
        notify::event::EventKind::Modify(notify::event::ModifyKind::Name(
            notify::event::RenameMode::From,
        )) => "REMOVE", // Renamed from old file
        notify::event::EventKind::Modify(notify::event::ModifyKind::Name(
            notify::event::RenameMode::Both,
        )) => "RENAME",
        notify::event::EventKind::Modify(_) => "WRITE", // Catch-all for other Modify kinds as WRITE
        notify::event::EventKind::Remove(_) => "REMOVE",
        _ => {
            debug!(
                "[WatcherThread] Unhandled or ignored debounced event kind: {:?} for path {}",
                debounced_event.kind, path_str
            );
            return;
        }
    };

    let event = Event {
        path: path_str.clone(),
        op: op_str.to_string(),
    };
    debug!("[WatcherThread] Produced event: {:?}", event);

    // Send the event to the main async event channel (tokio::sync::mpsc::Sender)
    // This is a blocking send because we are in a synchronous std::thread.
    // Ensure the receiving end in the async context is processing messages.
    if let Err(e) = event_tx.blocking_send(event.clone()) {
        error!(
            "[WatcherThread] Failed to send event to main async channel: {}. Event: {:?}",
            e, event
        );
    }
}
