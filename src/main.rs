mod cli;
mod image;

use clap::Parser;
use cli::{Cli, Commands, ImageSubcommands};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Image(img) => match img.command {
            ImageSubcommands::Cover(args) => image::cover::run(args)?,
        },
    }
    
    Ok(())
}
