use std::fmt::{Display, Formatter};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path(String);

#[derive(Debug, Error)]
pub enum PathError {
    #[error("Path must start with '/', got: {0}")]
    InvalidPath(String),
}

impl Path {
    /// Creates a new Path, ensuring it starts with '/'.
    /// Returns Err if the path doesn't start with '/'.
    pub fn new<T: Into<String>>(path: T) -> Result<Self, PathError> {
        let path_str = path.into();
        if path_str.starts_with('/') {
            Ok(Path(path_str))
        } else {
            Err(PathError::InvalidPath(path_str))
        }
    }

    /// Creates a new Path, automatically adding '/' prefix if missing.
    pub fn ensure_leading_slash<T: Into<String>>(path: T) -> Self {
        let path_str = path.into();
        if path_str.starts_with('/') {
            Path(path_str)
        } else {
            Path(format!("/{}", path_str))
        }
    }

    /// Get a string slice of the path
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for Path {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// Implement From for Path to handle string conversions
impl From<Path> for String {
    fn from(path: Path) -> Self {
        path.0
    }
}

impl From<&Path> for String {
    fn from(path: &Path) -> Self {
        path.0.clone()
    }
}

// Implement TryFrom for various string types
impl TryFrom<String> for Path {
    type Error = PathError;

    fn try_from(path: String) -> Result<Self, Self::Error> {
        Path::new(path)
    }
}

impl TryFrom<&String> for Path {
    type Error = PathError;

    fn try_from(path: &String) -> Result<Self, Self::Error> {
        Path::new(path)
    }
}

impl TryFrom<&str> for Path {
    type Error = PathError;

    fn try_from(path: &str) -> Result<Self, Self::Error> {
        Path::new(path)
    }
}

impl TryFrom<&&str> for Path {
    type Error = PathError;

    fn try_from(path: &&str) -> Result<Self, Self::Error> {
        Path::new(*path)
    }
}

// Reference conversion for Path
impl TryFrom<&Path> for Path {
    type Error = PathError;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        Ok(path.clone())
    }
}

impl AsRef<str> for Path {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_creation() {
        // Valid paths
        assert!(Path::new("/api/v1/test").is_ok());
        assert!(Path::new("/").is_ok());

        // Invalid paths
        assert!(Path::new("api/v1/test").is_err());
        assert!(Path::new("").is_err());
    }

    #[test]
    fn test_path_normalization() {
        assert_eq!(Path::ensure_leading_slash("/api/test").as_str(), "/api/test");
        assert_eq!(Path::ensure_leading_slash("api/test").as_str(), "/api/test");
    }

    #[test]
    fn test_string_conversion() {
        let path = Path::ensure_leading_slash("/test");
        let string: String = path.into();
        assert_eq!(string, "/test");

        let path = Path::ensure_leading_slash("/test");
        let string: String = (&path).into();
        assert_eq!(string, "/test");
    }
} 