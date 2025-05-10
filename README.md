# Hirai - Real-time File Change Broadcaster (Rust)

Hirai is a Rust utility that monitors specified directories for file system changes (create, write, remove, rename) and broadcasts these events over the local network using UDP multicast. It can also run in listener mode to receive and display these events, or provide a simple web interface for real-time monitoring in a browser.

## Features

*   **Directory Monitoring**: Monitors one or more directories for file system events (create, write, remove, rename).
*   **UDP Multicast Broadcasting**: Broadcasts detected file system events to a configurable UDP multicast group and port.
*   **Listener Mode**: Can run in a listener mode to receive and display events broadcast by other Hirai instances.
*   **Web UI**: Provides an optional web interface (accessible via `--browse`) that displays file change events in real-time using WebSockets.
*   **Configurable**: Supports configuration via a TOML file (`hirai.toml` by default), environment variables (prefixed with `HIRAI_`), and command-line arguments.
*   **Cross-Platform**: Built with Rust, aiming for cross-platform compatibility (Linux, macOS, Windows).

## Usage

### Installation

1.  Ensure you have Rust installed. If not, visit [rustup.rs](https://rustup.rs/).
2.  Clone the repository (or download the source code).
3.  Navigate to the project directory and build the project:

    ```bash
    cargo build --release
    ```
4.  The executable will be located at `target/release/hirai`.

### Broadcasting Changes

To monitor a directory (e.g., `/path/to/watch`) and broadcast changes:

```bash
./target/release/hirai /path/to/watch another/path
```

To specify a different multicast address:

```bash
./target/release/hirai --multicast "239.1.2.3:12345" /path/to/watch
```

### Listening for Changes

To listen for events on the default multicast address and print them to the console:

```bash
./target/release/hirai --listen
```

### Using the Web UI

To monitor a directory and enable the web UI (default: `http://0.0.0.0:8080`):

```bash
./target/release/hirai --browse /path/to/watch
```

Navigate to `http://0.0.0.0:8080` in your browser to see the events.

To run only the listener with the web UI (e.g., if another instance is broadcasting):

```bash
./target/release/hirai --listen --browse
```

### Configuration

Hirai can be configured using a `hirai.toml` file in the current directory, environment variables, or command-line arguments. Command-line arguments have the highest precedence, followed by environment variables, then the configuration file, and finally the built-in defaults.

**Important:**
- If you want to run both a broadcaster and a listener on the same machine, make sure only one process is started in listener mode (`listen = true`). If you start two processes with `listen = true` (either via config or CLI), you will get an "Address already in use" error because both try to bind to the same multicast port as listeners.
- The recommended approach is to set `listen = false` in your `hirai.toml` (or omit it), and use the `--listen` CLI flag for the listener process.

**Example hirai.toml for Broadcaster:**

```toml
[hirai]
folders = ["/path/to/monitor1", "/path/to/monitor2"]
multicast = "239.0.0.1:9999"
webaddr = "0.0.0.0:8080"
listen = false
browse = false
log_level = "info"
```

**Example hirai.toml for Listener:**

```toml
[hirai]
multicast = "239.0.0.1:9999"
webaddr = "0.0.0.0:8080"
listen = true
browse = false
log_level = "info"
```

**To run both a broadcaster and a listener:**

1. Start the broadcaster (default mode):
   ```bash
   ./target/release/hirai
   ```
   (Make sure `listen = false` in your `hirai.toml`)

2. Start the listener in a separate terminal:
   ```bash
   ./target/release/hirai --listen
   ```
   (This will override the config and start in listener mode)

**If you want to use two different config files:**
- You can specify the config file with `--config`:
  ```bash
  ./target/release/hirai --config hirai-broadcaster.toml
  ./target/release/hirai --config hirai-listener.toml
  ```

**Default Configuration (`hirai.toml`):**

```toml
# folders = ["/path/to/monitor1", "/path/to/monitor2"]
# multicast = "239.0.0.1:9999"
# webaddr = "0.0.0.0:8080"
# listen = false
# browse = false
# log_level = "info" # (trace, debug, info, warn, error)
```

**Environment Variables:**

*   `HIRAI_FOLDERS`: Comma-separated list of folders (e.g., `"/d1,/d2"`). Note: Figment expects specific formats for arrays from env.
*   `HIRAI_MULTICAST`: e.g., `"239.0.0.1:9999"`
*   `HIRAI_WEBADDR`: e.g., `"0.0.0.0:8080"`
*   `HIRAI_LISTEN`: `true` or `false`
*   `HIRAI_BROWSE`: `true` or `false`
*   `HIRAI_LOG_LEVEL`: `info`, `debug`, etc.

**Command-line Arguments:**

Run `./target/release/hirai --help` for a full list of options.

```
Usage: hirai [OPTIONS] [FOLDERS]...

Arguments:
  [FOLDERS]...
          One or more directories to monitor

Options:
  -l, --listen
          Run in listener mode (receive and print events)

  -b, --browse
          Enable web UI for monitoring changes

  -m, --multicast <MULTICAST>
          UDP multicast address (e.g., "239.0.0.1:9999")

  -w, --webaddr <WEBADDR>
          HTTP address for web UI (e.g., "0.0.0.0:8080")

  -c, --config <CONFIG>
          Path to a configuration file (e.g., hirai.toml)

      --log-level <LOG_LEVEL>
          Log level (e.g., trace, debug, info, warn, error)
          [env: HIRAI_LOG_LEVEL=]

  -h, --help
          Print help (see more with '--help')

  -V, --version
          Print version
```

## Development

### Building

```bash
cargo build
```

### Testing

Run unit and integration tests:

```bash
cargo test
```

### Linting and Formatting

```bash
cargo fmt
cargo clippy
```

## Project Structure

*   `src/main.rs`: Main application entry point, argument parsing, and task spawning.
*   `src/lib.rs`: Library entry point, exposes modules.
*   `src/config.rs`: Configuration loading and management (CLI, env, file).
*   `src/event.rs`: Defines the `Event` struct for file system changes.
*   `src/watcher.rs`: File system monitoring logic using `notify-debouncer-full`.
*   `src/network.rs`: UDP multicast broadcasting and listening logic.
*   `src/web.rs`: Web server and WebSocket handling for the UI using `axum`.
*   `static/index.html`: Simple HTML page for the real-time web UI.
*   `tests/integration_tests.rs`: Integration tests for the application.
*   `README.md`: This file.
*   `Cargo.toml`: Project manifest and dependencies.

## Potential Future Enhancements (Not Implemented)

*   **Encrypted Broadcasts**: Option to encrypt UDP payloads for secure communication, perhaps using a pre-shared key.
*   **Event Filtering**: Allow users to specify patterns (e.g., glob patterns) to include or exclude certain files/directories from being monitored or broadcasted.
*   **More Sophisticated Web UI**: Enhance the web UI with features like event filtering, history, and better visualization.
*   **Pluggable Notifiers**: Beyond UDP multicast, support other notification mechanisms (e.g., HTTP callbacks, message queues like Kafka/RabbitMQ).
*   **Persistent Event Log**: Option to log events to a file or a simple database for auditing or later review.
*   **Dockerization**: Provide a Dockerfile for easy deployment.
*   **Performance Profiling and Optimization**: For very high-volume file changes, further profiling and optimization might be beneficial.
*   **Configuration for Debouncer**: Allow tuning of debouncer parameters (e.g., delay) via configuration.
*   **Specific Rename Event Handling**: The `notify` crate can provide more detailed rename events (`ModifyKind::Name(RenameMode::Any)` etc.). The current implementation simplifies renames into CREATE/REMOVE based on debouncer output. A more direct RENAME event could be implemented.

## License

This project is dual-licensed under either [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE) license, at your option.
