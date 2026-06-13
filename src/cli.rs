use clap::{Parser, Subcommand};
use crate::image;

#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Image(ImageCommands),
}

#[derive(Parser)]
pub struct ImageCommands {
    #[command(subcommand)]
    pub command: ImageSubcommands,
}

#[derive(Subcommand)]
pub enum ImageSubcommands {
    Cover(image::cover::CoverArgs),
}
