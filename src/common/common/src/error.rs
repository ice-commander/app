use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    Cancelled,
    Io(String),
    Remote(String),
    Config(String),
    Other(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Cancelled => write!(f, "Cancelled"),
            AppError::Io(msg) => write!(f, "{}", msg),
            AppError::Remote(msg) => write!(f, "{}", msg),
            AppError::Config(msg) => write!(f, "{}", msg),
            AppError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for AppError {}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError::Other(s)
    }
}

impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        AppError::Other(s.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::Interrupted {
            AppError::Cancelled
        } else {
            AppError::Io(e.to_string())
        }
    }
}

impl From<AppError> for String {
    fn from(e: AppError) -> Self {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_displays_as_cancelled() {
        assert_eq!(AppError::Cancelled.to_string(), "Cancelled");
    }

    #[test]
    fn io_displays_inner_message() {
        assert_eq!(AppError::Io("disk full".into()).to_string(), "disk full");
    }

    #[test]
    fn remote_displays_inner_message() {
        assert_eq!(AppError::Remote("timeout".into()).to_string(), "timeout");
    }

    #[test]
    fn config_displays_inner_message() {
        assert_eq!(AppError::Config("bad key".into()).to_string(), "bad key");
    }

    #[test]
    fn other_displays_inner_message() {
        assert_eq!(AppError::Other("whoops".into()).to_string(), "whoops");
    }

    #[test]
    fn from_string_becomes_other() {
        assert_eq!(AppError::from("oops".to_string()), AppError::Other("oops".into()));
    }

    #[test]
    fn from_str_becomes_other() {
        assert_eq!(AppError::from("oops"), AppError::Other("oops".into()));
    }

    #[test]
    fn from_io_interrupted_becomes_cancelled() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Interrupted, "interrupted");
        assert_eq!(AppError::from(io_err), AppError::Cancelled);
    }

    #[test]
    fn from_io_not_found_becomes_io_variant() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        assert!(matches!(AppError::from(io_err), AppError::Io(_)));
    }

    #[test]
    fn from_io_permission_denied_becomes_io_variant() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        assert!(matches!(AppError::from(io_err), AppError::Io(_)));
    }

    #[test]
    fn into_string_uses_display() {
        let msg = "something failed";
        let s: String = AppError::Remote(msg.into()).into();
        assert_eq!(s, msg);
    }

    #[test]
    fn cancelled_into_string() {
        let s: String = AppError::Cancelled.into();
        assert_eq!(s, "Cancelled");
    }

    #[test]
    fn clone_eq() {
        let e = AppError::Config("bad creds".into());
        assert_eq!(e.clone(), e);
    }

    #[test]
    fn different_variants_not_equal() {
        assert_ne!(AppError::Io("x".into()), AppError::Remote("x".into()));
    }
}
