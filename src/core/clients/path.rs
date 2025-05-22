use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path(String);

impl Path {
    pub fn ensure_leading_slash<T: Into<String>>(path: T) -> String {
        let path_str = path.into();
        if path_str.starts_with('/') {
            path_str
        } else {
            format!("/{}", path_str)
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for Path {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

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

impl From<&str> for Path {
    fn from(path: &str) -> Self {
        Path(Path::ensure_leading_slash(path))
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
        // Test paths with leading slash are preserved
        assert_eq!(Path::from("/api/v1/test").as_str(), "/api/v1/test");
        assert_eq!(Path::from("/").as_str(), "/");

        // Test paths without leading slash have it added
        assert_eq!(Path::from("api/v1/test").as_str(), "/api/v1/test");
        assert_eq!(Path::from("").as_str(), "/");
    }

    #[test]
    fn test_path_normalization() {
        // Test normalization adds leading slash consistently
        assert_eq!(Path::from("/api/test").as_str(), "/api/test");
        assert_eq!(Path::from("api/test").as_str(), "/api/test");
    }

    #[test]
    fn test_string_conversion() {
        // Test owned Path to String conversion
        let path = Path::from("/test");
        let string: String = path.into();
        assert_eq!(string, "/test");

        // Test Path reference to String conversion
        let path = Path::from("/test");
        let string: String = (&path).into();
        assert_eq!(string, "/test");
    }

    #[test]
    fn test_display() {
        // Test Display implementation
        let path = Path::from("/foo/bar");
        assert_eq!(format!("{}", path), "/foo/bar");
    }

    #[test]
    fn test_as_ref_str() {
        // Test AsRef<str> implementation
        let path = Path::from("foo/bar");
        assert_eq!(path.as_ref(), "/foo/bar");
    }

    #[test]
    fn test_clone_and_eq() {
        // Test Clone and PartialEq implementations
        let path1 = Path::from("/test");
        let path2 = path1.clone();
        assert_eq!(path1, path2);
    }
}
