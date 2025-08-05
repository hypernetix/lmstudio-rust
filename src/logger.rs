//! Logging functionality for the LM Studio Rust client

use std::fmt;

/// Log levels matching the Go implementation
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogLevel::Error => write!(f, "ERROR"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Debug => write!(f, "DEBUG"),
        }
    }
}

/// Logger implementation matching the Go interface
#[derive(Debug, Clone)]
pub struct Logger {
    level: LogLevel,
    prefix: String,
}

impl Logger {
    /// Create a new logger with the specified log level
    pub fn new(level: LogLevel) -> Self {
        Self {
            level,
            prefix: "[LMStudio]".to_string(),
        }
    }

    /// Create a new logger with a custom prefix
    pub fn with_prefix<S: Into<String>>(level: LogLevel, prefix: S) -> Self {
        Self {
            level,
            prefix: prefix.into(),
        }
    }

    /// Set the log level
    pub fn set_level(&mut self, level: LogLevel) {
        self.level = level;
    }

    /// Get the current log level
    pub fn level(&self) -> LogLevel {
        self.level
    }

    /// Check if a log level is enabled
    pub fn is_enabled(&self, level: LogLevel) -> bool {
        level <= self.level
    }

    /// Log an error message
    pub fn error<S: AsRef<str>>(&self, message: S) {
        if self.is_enabled(LogLevel::Error) {
            eprintln!("{} [ERROR] {}", self.prefix, message.as_ref());
        }
    }

    /// Log a warning message
    pub fn warn<S: AsRef<str>>(&self, message: S) {
        if self.is_enabled(LogLevel::Warn) {
            println!("{} [WARN] {}", self.prefix, message.as_ref());
        }
    }

    /// Log an info message
    pub fn info<S: AsRef<str>>(&self, message: S) {
        if self.is_enabled(LogLevel::Info) {
            println!("{} [INFO] {}", self.prefix, message.as_ref());
        }
    }

    /// Log a debug message
    pub fn debug<S: AsRef<str>>(&self, message: S) {
        if self.is_enabled(LogLevel::Debug) {
            println!("{} [DEBUG] {}", self.prefix, message.as_ref());
        }
    }

    /// Log a formatted error message
    pub fn errorf(&self, format: &str, args: &[&dyn fmt::Display]) {
        if self.is_enabled(LogLevel::Error) {
            let message = format_message(format, args);
            eprintln!("{} [ERROR] {}", self.prefix, message);
        }
    }

    /// Log a formatted warning message
    pub fn warnf(&self, format: &str, args: &[&dyn fmt::Display]) {
        if self.is_enabled(LogLevel::Warn) {
            let message = format_message(format, args);
            println!("{} [WARN] {}", self.prefix, message);
        }
    }

    /// Log a formatted info message
    pub fn infof(&self, format: &str, args: &[&dyn fmt::Display]) {
        if self.is_enabled(LogLevel::Info) {
            let message = format_message(format, args);
            println!("{} [INFO] {}", self.prefix, message);
        }
    }

    /// Log a formatted debug message
    pub fn debugf(&self, format: &str, args: &[&dyn fmt::Display]) {
        if self.is_enabled(LogLevel::Debug) {
            let message = format_message(format, args);
            println!("{} [DEBUG] {}", self.prefix, message);
        }
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::new(LogLevel::Error)
    }
}

/// Helper function to format messages with arguments
fn format_message(format: &str, args: &[&dyn fmt::Display]) -> String {
    let mut result = format.to_string();
    for (i, arg) in args.iter().enumerate() {
        let placeholder = format!("{{{}}}", i);
        result = result.replace(&placeholder, &arg.to_string());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_levels() {
        assert!(LogLevel::Error < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Debug);
    }

    #[test]
    fn test_logger_creation() {
        let logger = Logger::new(LogLevel::Info);
        assert_eq!(logger.level(), LogLevel::Info);
        assert!(logger.is_enabled(LogLevel::Error));
        assert!(logger.is_enabled(LogLevel::Warn));
        assert!(logger.is_enabled(LogLevel::Info));
        assert!(!logger.is_enabled(LogLevel::Debug));
    }

    #[test]
    fn test_format_message() {
        let result = format_message("Hello {0}, you have {1} messages", &[&"Alice", &42]);
        assert_eq!(result, "Hello Alice, you have 42 messages");
    }
}
