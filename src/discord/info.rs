use serenity::framework::standard::macros::group;
use crate::discord::commands::{
    HEIGHT_COMMAND,
    UPTIME_COMMAND,
    STATUS_COMMAND,
    HELP_COMMAND
};

#[group]
#[commands(uptime,height,status,help)]
pub struct DiscordInfo;
