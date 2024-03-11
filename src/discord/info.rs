use serenity::framework::standard::macros::group;
use crate::discord::commands::{
    HEIGHT_COMMAND,
    HELP_COMMAND
};

#[group]
#[commands(height,help)]
pub struct DiscordInfo;
