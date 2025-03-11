use crate::{console, ConnectionErrorMessage, ConnectionErrorType, VERSION};
use anyhow::{anyhow, Context as _, Result};
use tokio_tungstenite::tungstenite::Error as WsError;

/// Handle WebSocket errors
pub fn handle_ws_error(err: WsError) -> Result<()> {
    match err {
        // In case of Bad Request
        WsError::Http(res) if res.status() == 400 => {
            let result: Result<()> = 'tryblock: {
                // Get the response body
                let header = match res
                    .headers()
                    .get("X-Error")
                    .context("Connection refused without error message")
                {
                    Ok(header) => header,
                    Err(err) => break 'tryblock Err(err),
                };
                let text = match header
                    .to_str()
                    .context("Connection refused with invalid error message")
                {
                    Ok(text) => text,
                    Err(err) => break 'tryblock Err(err),
                };
                // Parse JSON
                let ConnectionErrorMessage { message, error } =
                    match serde_json::from_str::<ConnectionErrorMessage>(text) {
                        Ok(json) => json,
                        Err(err) => break 'tryblock Err(err.into()),
                    };
                // If parsing is successful
                match error {
                    // If the version is outdated
                    ConnectionErrorType::Outdated { required, download } => {
                        // Display the content
                        if let Err(err) = console::printdoc! {"

                            ↑ Update required: {VERSION} to {required}
                              Download: {download}

                            "}
                        {
                            break 'tryblock Err(err);
                        }

                        // Open the browser
                        let _ = webbrowser::open(&download);
                    }
                    // For other errors
                    _ => {
                        if let Some(message) = message {
                            // Indent the message
                            let message = message
                                .lines()
                                .map(|line| format!("  {}", line))
                                .collect::<Vec<String>>()
                                .join("\n");

                            // Display the error message
                            if let Err(err) = console::printdoc! {
                                "

                                    ☓ Connection error:
                                    {message}

                                    "
                            } {
                                break 'tryblock Err(err);
                            }
                        }
                    }
                }

                Ok(())
            };

            if let Err(err) = result {
                // If parsing fails
                console::eprintln!("☓ {err}")?;
            }
        }
        // For other HTTP errors
        WsError::Http(res) => Err(anyhow!("HTTP error: {}", res.status()))?,
        // For other errors
        _ => Err(err).context("Failed to connect to the server")?,
    }

    Ok(())
}
