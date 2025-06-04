use lazy_static::lazy_static;
use prometheus::Registry;

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
}
