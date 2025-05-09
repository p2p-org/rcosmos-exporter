use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct NotarySessionResponse {
    pub notary_sessions: Vec<NotarySession>,
    pub pagination: Option<Pagination>,
}

#[derive(Deserialize, Debug)]
pub struct Pagination {
    pub next_key: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct NotarySession {
    pub id: String,
    pub signatures: Vec<Option<String>>,
    pub val_set: ValSet,
}

#[derive(Deserialize, Debug)]
pub struct ValSet {
    pub participants: Vec<Participant>,
}

#[derive(Deserialize, Debug)]
pub struct Participant {
    pub operator: String,
}
