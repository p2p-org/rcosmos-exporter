use std::sync::Arc;
use serenity::{
    framework::standard::{macros::command, CommandResult},
    model::channel::Message,
    prelude::*,
};
use discord_bot::tendermint::rpc::*;
use discord_bot::MessageLog;
use discord_bot::internal::logger::JsonLog;
use crate::WATCHER_CLIENT;

const DESCRIPTION: &str = r#"Discord Bot for Anoma Namada-Shielded Expedition
This bot provides assistance and information related to the Anoma Namada-Shielded Expedition uptime by your validator.
It offers few main commands:

- `$height`: Checks the current height of the Chain.
- `$uptime`: Show the current uptime for registered Validator address.
- `$status`: Shout yay if the bot is alive.
- `$help`: Displays a multi-line description of the discard bot."#;

#[command]
pub async fn help(ctx: &Context, msg: &Message) -> CommandResult {
    msg.channel_id.say(&ctx.http, DESCRIPTION).await?;
    Ok(())
}

#[command]
pub async fn status(ctx: &Context, msg: &Message) -> CommandResult {
    msg.channel_id.say(&ctx.http, "***YAY***, I'm alive").await?;
    Ok(())
}

#[command]
pub async fn height(ctx: &Context, msg: &Message) -> CommandResult {
    let ctx_arc = Arc::new(ctx.clone());
    let msg_channel_id = msg.channel_id;

    let task = async move {
        let rpc_client = RPC_CLIENT.lock().unwrap().clone();
        match rpc_client {
            Some(rpc) => {
                if let Ok(height_response) = rpc.get_block(0).await {
                    match height_response.result.block.header.height.parse::<i64>() {
                        Ok(parsed_height) => {
                            let _ = msg_channel_id.say(&ctx_arc.http, format!(
                                "The current height of Shielded expedition: ***{}***",
                                parsed_height
                            )).await;
                        }
                        Err(_) => {
                            let _ = msg_channel_id.say(&ctx_arc.http, "Error: Failed to parse block height.").await;
                        }
                    }
                } else {
                    let _ = msg_channel_id.say(&ctx_arc.http, "Error: Failed to get last block.").await;
                }
            }
            None => {
                let _ = msg_channel_id.say(&ctx_arc.http, "Error: RPC client not initialized.").await;
            }
        }
    };

    tokio::spawn(task);

    Ok(())
}

#[command]
pub async fn uptime(ctx: &Context, msg: &Message) -> CommandResult {
    let msg_channel_id = msg.channel_id;
    let watcher_arc = {
        let watcher = WATCHER_CLIENT.lock().unwrap();
        match &*watcher {
            Some(watcher_instance) => {
                Some(Arc::clone(watcher_instance))
            },
            None => {
                MessageLog!("Watcher is not initialized");
                return Err("Watcher client is not initialized".into());
            }
        }
    };

    if let Some(watcher) = watcher_arc {
        let uptime = watcher.get_uptime();
        msg_channel_id.say(&ctx.http, format!("Your Uptime: ***{:.2}%***", uptime)).await?;
    }
    Ok(())
}