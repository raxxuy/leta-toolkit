mod cli;
mod image;
mod screenshot;

use clap::Parser;
use cli::{Cli, Commands, ImageSubcommands, ScreenshotSubcommands};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Image(img) => match img.command {
            ImageSubcommands::Cover(args) => image::cover::run(args)?,
        },
        Commands::Screenshot(s) => match s.command {
            ScreenshotSubcommands::Capture(args) => screenshot::capture::run(args)?,
        },
    }

    Ok(())
}
