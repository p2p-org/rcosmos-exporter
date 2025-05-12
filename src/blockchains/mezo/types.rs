#![allow(unused_variables)]
#![allow(dead_code)]

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct MezoRESTResponse {
    pub validators: Vec<MezoRESTValidator>,
}

#[derive(Debug, Deserialize)]
pub struct TendermintMezoPagination {
    pub next_key: Option<String>,
    pub total: String,
}

#[derive(Debug, Deserialize)]
pub struct MezoRESTValidator {
    pub cons_pub_key_bech32: String,
    pub description: MezoRESTDescription,
}

#[derive(Debug, Deserialize)]
pub struct MezoRESTDescription {
    pub moniker: String,
}
