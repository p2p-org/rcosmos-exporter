use std::fmt;

pub struct JsonLog {
    pub message: String,
}

impl fmt::Debug for JsonLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{ \"message\": \"{}\" }}", self.message)
    }
}
