#![allow(unused_variables)]
#![allow(dead_code)]

use serde::Deserialize;
use serde_json::Value;

/*
    BLS  data types
*/
#[derive(Deserialize)]
pub struct CurrentEpoch {
    pub current_epoch: String,
    pub epoch_boundary: String,
}

#[derive(Deserialize)]
pub struct GetEpochResponse {
    pub epoch: Epoch,
}

#[derive(Deserialize)]
pub struct Epoch {
    pub epoch_number: String,
    pub current_epoch_interval: String,
    pub first_block_height: String,
    pub last_block_time: String,
    pub sealer_app_hash_hex: String,
    pub sealer_block_hash: String,
}

#[derive(Deserialize)]
pub struct BlockTxs {
    pub txs: Vec<Tx>,
}

#[derive(Deserialize)]
pub struct Tx {
    pub body: TxBody,
}

#[derive(Deserialize)]
pub struct TxBody {
    pub messages: Vec<TxMessage>,
}

#[derive(Deserialize)]
pub struct TxMessage {
    pub extended_commit_info: ExtendedCommitInfo,

    #[serde(flatten)]
    _ignored: Value, // Catches everything else but does not use it
}

#[derive(Deserialize)]
pub struct ExtendedCommitInfo {
    pub round: usize,
    pub votes: Vec<Vote>,
}

#[derive(Deserialize)]
pub struct Vote {
    pub validator: Validator,
    pub vote_extension: Option<String>,
    pub extension_signature: Option<String>,
    pub block_id_flag: String,
}

#[derive(Deserialize)]
pub struct Validator {
    pub address: String,
    pub power: String,
}
