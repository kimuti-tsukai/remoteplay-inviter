use anyhow::{Context as _, Result};
use crossterm::{cursor, terminal, QueueableCommand};
use std::fmt::Arguments;
use std::io::{self, Write as _};
use std::sync::{LazyLock, Mutex};

/// Last line
static LAST_LINE: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new("".to_string()));

/// Clears the current line
pub fn clear_line() -> Result<()> {
    io::stdout()
        .queue(terminal::Clear(terminal::ClearType::CurrentLine))
        .context("Failed to update output (clear line)")?;
    Ok(())
}

/// Saves the last line
pub fn save_line(args: std::fmt::Arguments<'_>) -> Result<()> {
    // Save the last line
    let mut data = LAST_LINE
        .lock()
        .map_err(|_| anyhow::anyhow!("Failed to lock last line"))?;
    *data = std::fmt::format(args);
    Ok(())
}

/// Updates the current line
/// <https://stackoverflow.com/a/59890400>
pub fn update_line() -> Result<()> {
    let mut stdout = io::stdout();
    let data = LAST_LINE
        .lock()
        .map_err(|_| anyhow::anyhow!("Failed to lock last line"))?;
    stdout
        .queue(terminal::Clear(terminal::ClearType::CurrentLine))
        .context("Failed to update output (clear line)")?;
    stdout
        .write_all(data.as_bytes())
        .context("Failed to update output (write)")?;
    stdout
        .queue(cursor::MoveToColumn(0))
        .context("Failed to update output (left feed)")?;
    stdout.flush().context("Failed to update output (flush)")?;
    Ok(())
}

pub(crate) fn fn_println(args: std::fmt::Arguments<'_>) -> Result<()> {
    clear_line()?;
    io::stdout().write_fmt(args)?; // Call the original macro
    update_line()?;
    Ok(())
}

/// println macro
macro_rules! println {
    ($($arg:tt)*) => {{
        $crate::console::fn_println(format_args!($($arg)*))
    }};
}
pub(crate) use println;

pub(crate) fn fn_eprintln(args: Arguments) -> Result<()> {
    clear_line()?;
    io::stderr().write_fmt(args)?;
    update_line()?;
    Ok(())
}

/// eprintln macro
macro_rules! eprintln {
    ($($arg:tt)*) => {{
        $crate::console::fn_eprintln(format_args!($($arg)*))
    }};
}
pub(crate) use eprintln;

/// printdoc macro
macro_rules! printdoc {
    ($($arg:tt)*) => {{
        'aaa: {
            if let Err(e) = $crate::console::clear_line() {
                break 'aaa Err(e);
            }

            ::indoc::printdoc!($($arg)*);

            if let Err(e) = $crate::console::update_line() {
                break 'aaa Err(e);
            }

            Ok(())
        }
    }};
}
pub(crate) use printdoc;

pub(crate) fn fn_print_update(args: Arguments) -> Result<()> {
    save_line(args)?;
    update_line()?;
    Ok(())
}

/// print_update macro
macro_rules! print_update {
    ($($arg:tt)*) => {{
        $crate::console::fn_print_update(format_args!($($arg)*))
    }};
}
pub(crate) use print_update;
