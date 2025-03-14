use anyhow::{Context as _, Result};
use dotenvy_macro::dotenv;
use futures::SinkExt;
use futures_util::stream::StreamExt;
use std::{borrow::Cow, sync::Arc};
use steam_stuff::SteamStuff;
use tokio::{
    sync::Mutex,
    time::{self, timeout, Duration},
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        http::{uri::Builder, Uri},
        protocol::Message,
    },
};
use uuid::Uuid;

mod config;
mod console;
mod handlers;
mod models;
mod retry;
mod ws_error_handler;

use config::{read_or_generate_config, Config};
use handlers::Handler;
use models::*;
use retry::RetrySec;
use ws_error_handler::handle_ws_error;

// Version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// Endpoint URL
const DEFAULT_URL: &str = dotenv!("ENDPOINT_URL");

#[tokio::main]
async fn main() -> Result<()> {
    // Event loop
    'main: {
        console::printdoc! {"
            ------------------------------------------------------------------------------
                        ╦═╗┌─┐┌┬┐┌─┐┌┬┐┌─┐┌─┐┬  ┌─┐┬ ┬  ╦┌┐┌┬  ┬┬┌┬┐┌─┐┬─┐
                        ╠╦╝├┤ ││││ │ │ ├┤ ├─┘│  ├─┤└┬┘  ║│││└┐┌┘│ │ ├┤ ├┬┘
                        ╩╚═└─┘┴ ┴└─┘ ┴ └─┘┴  ┴─┘┴ ┴ ┴   ╩┘└┘ └┘ ┴ ┴ └─┘┴└─
                           Version: {VERSION}                   by Kamesuta

                Invite your friends via Discord and play Steam games together for free!
            ------------------------------------------------------------------------------

        "}?;

        // Version command
        if std::env::args().any(|arg| arg == "--version" || arg == "-v") {
            console::println!("✓ Version: {}", VERSION)?;
            return Ok(());
        }

        // Help command
        if std::env::args().any(|arg| arg == "--help" || arg == "-h") {
            let program = std::env::current_exe()
                .ok()
                .and_then(|f| f.file_name().map(|f| f.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "remoteplay-inviter".to_owned());
            console::printdoc! {"
                Usage: {program} [options]

                Options:
                    -v, --version    Display the version of the program
                    -h, --help       Display this help message
            "}?;
            return Ok(());
        }

        // Initialize SteamStuff
        let steam = match SteamStuff::new()
            .context("Failed to connect to Steam Client. Please make sure Steam is running.")
        {
            Ok(steam) => Arc::new(Mutex::new(steam)),
            Err(err) => {
                console::eprintln!("☓ {}", err)?;
                break 'main;
            }
        };

        // Create a Handler
        let mut handler = Handler::new(steam.clone());

        // Set up Steam callbacks
        handler.setup_steam_callbacks().await;
        // Start a task to periodically call Steam callbacks
        handler.run_steam_callbacks();

        // Reconnection flag
        let mut reconnect = false;
        // Retry seconds
        let mut retry_sec = RetrySec::new();

        // URL to connect to
        let result: Result<String> = 'tryblock: {
            // Read the endpoint configuration file
            let endpoint_config = match config::read_endpoint_config() {
                Ok(config) => config,
                Err(err) => {
                    break 'tryblock Err(err);
                }
            };

            // Read or generate the configuration file (if it doesn't exist)
            let config = match read_or_generate_config(|| Config {
                uuid: Uuid::new_v4().to_string(),
            }) {
                Ok(config) => config,
                Err(err) => {
                    break 'tryblock Err(err);
                }
            };

            // Session ID
            let session_id: u32 = rand::random();

            // Endpoint URL
            let endpoint_url: Cow<'_, str> = match endpoint_config {
                Some(e) => {
                    if let Err(err) = console::println!("✓ Using custom endpoint URL: {}", e.url)
                    {
                        break 'tryblock Err(err);
                    }
                    e.url.into()
                }
                None => DEFAULT_URL.into(),
            };

            // Create the URL
            let uri: Uri = match endpoint_url.parse().context("Failed to parse URL") {
                Ok(uri) => uri,
                Err(err) => {
                    break 'tryblock Err(err);
                }
            };
            let uri = match Builder::from(uri)
                .path_and_query(format!(
                    "/ws?v={VERSION}&token={0}&session={session_id}",
                    config.uuid
                ))
                .build()
                .context("Failed to build URL")
            {
                Ok(uri) => uri,
                Err(err) => {
                    break 'tryblock Err(err);
                }
            };
            Ok(uri.to_string())
        };
        let url = match result {
            Ok(url) => url,
            Err(err) => {
                console::eprintln!("☓ {}", err)?;
                break 'main;
            }
        };

        loop {
            let result: Result<()> = 'tryblock: {
                // Display the reconnection message
                if reconnect {
                    if let Err(err) = console::println!("↪ Reconnecting to the server...") {
                        break 'tryblock Err(err);
                    }
                }

                // Create a WebSocket client
                let connect_result = match timeout(Duration::from_secs(10), connect_async(&url))
                    .await
                    .context("Connection timed out to the server")
                {
                    Ok(r) => r,
                    Err(err) => {
                        break 'tryblock Err(err);
                    }
                };
                let ws_stream = match connect_result {
                    Ok((ws_stream, _)) => ws_stream,
                    Err(err) => {
                        if let Err(err) = handle_ws_error(err) {
                            break 'tryblock Err(err);
                        }
                        // If OK is returned, break the loop and exit
                        break 'main;
                    }
                };

                // Stream and sink for communicating with the server
                let (mut write, mut read) = ws_stream.split();

                // Display the reconnection message
                if let Err(err) = if reconnect {
                    console::println!("✓ Reconnected!")
                } else {
                    console::println!("✓ Connected to the server!")
                } {
                    break 'tryblock Err(err);
                }

                // Loop to process messages received from the server
                while let Some(message) = {
                    match timeout(Duration::from_secs(60), read.next())
                        .await
                        .context("Connection timed out")
                    {
                        Ok(message) => message,
                        Err(err) => {
                            break 'tryblock Err(err);
                        }
                    }
                } {
                    // Process each message
                    match message.context("Failed to receive message from the server") {
                        Ok(Message::Close(_)) => break,
                        Ok(Message::Ping(ping)) => {
                            // Send a Pong message
                            if let Err(err) = write
                                .send(Message::Pong(ping))
                                .await
                                .context("Failed to send pong message to the server")
                            {
                                break 'tryblock Err(err);
                            }

                            // Reset the retry seconds
                            retry_sec.reset();
                        }
                        Ok(Message::Text(text)) => {
                            // Parse the JSON data
                            let msg: ServerMessage = match serde_json::from_str(&text) {
                                Ok(msg) => msg,
                                Err(err) => break 'tryblock Err(err.into()),
                            };

                            // Process the message
                            match handler.handle_server_message(msg, &mut write).await {
                                // If the exit flag is set, break the loop and exit
                                Ok(true) => break 'main,
                                Ok(false) => (),
                                Err(err) => break 'tryblock Err(err),
                            }

                            // Reset the retry seconds
                            retry_sec.reset();
                        }
                        Ok(_) => (),
                        Err(err) => break 'tryblock Err(err),
                    }
                }

                Ok(())
            };
            if let Err(err) = result {
                console::eprintln!("☓ {}", err)?;
            }

            // Reconnect to the server if the connection is lost
            let sec = retry_sec.next();
            console::println!("↪ Connection lost. Reconnecting in {sec} seconds...")?;
            time::sleep(Duration::from_secs(sec)).await;
            reconnect = true;
        }
    }

    // Wait for input before exiting
    console::println!("□ Press Ctrl+C to exit...")?;
    let _ = tokio::signal::ctrl_c().await;

    Ok(())
}
