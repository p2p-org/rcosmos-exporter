use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct BlockResponse {
    pub result: BlockResult,
}

#[derive(Debug, Deserialize)]
pub struct BlockResult {
    pub block: Block,
}

#[derive(Debug, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub last_commit: LastCommit,
}

#[derive(Debug, Deserialize)]
pub struct BlockHeader {
    pub height: String,
}

#[derive(Debug, Deserialize)]
pub struct LastCommit {
    pub signatures: Vec<CommitSignature>,
}

#[derive(Debug, Deserialize)]
pub struct CommitSignature {
    pub block_id_flag: i64,
    pub validator_address: String,
}
