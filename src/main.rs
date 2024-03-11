extern crate dotenv;

mod config;
mod discord;

use discord::establish::discord_client;
use discord_bot::tendermint::rpc::initialize_rpc_client;

#[tokio::main]
async fn main() {
    // Call the discord_client function to initialize the client
    initialize_rpc_client().await;
    let mut client = discord_client().await;

    // Start the client
    if let Err(why) = client.start().await {
        eprintln!("An error occurred while running the client: {:?}", why);
    }
}

