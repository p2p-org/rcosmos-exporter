/// Shared utilities for JSON parsing and response handling across blockchain implementations
use serde_json::Value;

/// Response structure variants for different blockchain implementations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseStructure {
    /// CometBFT-style: {"result": {"node_info": {...}, ...}}
    CometBft,
    /// Sei-style: {"node_info": {...}, ...} (no result wrapper)
    Sei,
}

/// Detect the response structure by checking for a "result" wrapper
pub fn detect_response_structure(json: &Value) -> ResponseStructure {
    if json.get("result").is_some() {
        ResponseStructure::CometBft
    } else {
        ResponseStructure::Sei
    }
}

/// Extract tx_index value from status response, handling both CometBFT and Sei structures
/// Returns Some("on") if tx_index is enabled, None otherwise
pub fn extract_tx_index(json: &Value) -> Option<&str> {
    let structure = detect_response_structure(json);

    let tx_index_opt = match structure {
        ResponseStructure::CometBft => {
            json.get("result")?
                .get("node_info")?
                .get("other")?
                .get("tx_index")?
                .as_str()
        }
        ResponseStructure::Sei => {
            json.get("node_info")?
                .get("other")?
                .get("tx_index")?
                .as_str()
        }
    };

    tx_index_opt.filter(|&v| v == "on")
}

/// Extract transactions array from tx_search response, handling multiple response formats
/// Supports: {"result": {"txs": [...]}}, {"txs": [...]}, and direct array responses
pub fn extract_txs_from_response(json: &Value) -> Option<&Value> {
    // Try CometBFT-style: result.txs
    if let Some(txs) = json.get("result").and_then(|r| r.get("txs")) {
        return Some(txs);
    }

    // Try direct txs field
    if let Some(txs) = json.get("txs") {
        return Some(txs);
    }

    // Try if the whole response is an array (unlikely but possible)
    if json.is_array() {
        return Some(json);
    }

    None
}


/// Create a preview string for error logging (truncates long responses)
pub fn create_error_preview(response: &str, max_len: usize) -> String {
    if response.len() > max_len {
        format!("{}...", &response[..max_len])
    } else {
        response.to_string()
    }
}

/// Recursively search for a key in nested JSON, regardless of nesting level
/// This is useful for finding fields that may be at different nesting levels
pub fn find_nested_value<'a>(json: &'a Value, key: &str) -> Option<&'a Value> {
    match json {
        Value::Object(map) => {
            // Check if this level has the key
            if let Some(value) = map.get(key) {
                return Some(value);
            }
            // Recursively search in all object values
            for value in map.values() {
                if let Some(found) = find_nested_value(value, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => {
            // Search in array elements
            for value in arr {
                if let Some(found) = find_nested_value(value, key) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}
