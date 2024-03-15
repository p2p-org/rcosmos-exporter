extern crate dotenv;

mod config;
mod discord;

use discord::establish::discord_client;
use discord_bot::tendermint::rpc::initialize_rpc_client;
use discord_bot::tendermint::watcher::{initialize_watcher_client, WATCHER_CLIENT};

#[tokio::main]
async fn main() {
    initialize_rpc_client().await;
    initialize_watcher_client().await.expect("Failed to initialize watcher client");
    let mut client = discord_client().await;

    if let Err(why) = client.start().await {
        eprintln!("An error occurred while running the client: {:?}", why);
    }
}

