use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SeiValidatorsResponse {
    pub count: String,
    pub total: String,
    pub validators: Vec<SeiValidator>,
}

#[derive(Debug, Deserialize)]
pub struct SeiValidator {
    pub address: String,
    pub voting_power: String,
    pub proposer_priority: String,
}

#[derive(Debug, Deserialize)]
pub struct SeiTxResponse {
    pub txs: Vec<SeiTx>,
}

#[derive(Debug, Deserialize)]
pub struct SeiTx {
    // We only need the count of transactions, not the individual tx data
}
