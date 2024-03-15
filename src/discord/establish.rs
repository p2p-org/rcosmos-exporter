use serenity::{
    async_trait,
    client::Client,
    framework::standard::{
        StandardFramework,
        Configuration
    },
    model::gateway::GatewayIntents,
    prelude::EventHandler,
};

use discord_bot::MessageLog;
use discord_bot::internal::logger::JsonLog;

use crate::{
    config,
    discord::{
        info::DISCORDINFO_GROUP,
    },
};

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: serenity::client::Context, ready: serenity::model::gateway::Ready) {
        MessageLog!("Discord bot {} is established and ready to listen the commands", ready.user.name);
    }
}

pub fn get_discord_token(settings: &config::Settings) -> &str {
    settings.discord_token()
}

pub async fn discord_client() -> Client {
    let settings = config::Settings::new();

    let framework = StandardFramework::new().group(&DISCORDINFO_GROUP);
    framework.configure(Configuration::new().prefix("$"));

    let token = match settings {
        Ok(settings) => get_discord_token(&settings).to_string(),
        Err(err) => {
            MessageLog!("Error parsing settings: {:?}", err);
            "default_token".to_string()
        }
    };
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let client = Client::builder(&token, intents)
        .event_handler(Handler)
        .framework(framework)
        .await
        .expect("Error creating client");

    client
}
