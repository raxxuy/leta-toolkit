use crate::image;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Image(ImageCommands),
    Screenshot(ScreenshotCommands),
    Palette(crate::palette::generate::PaletteArgs),
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

#[derive(Parser)]
pub struct ScreenshotCommands {
    #[command(subcommand)]
    pub command: ScreenshotSubcommands,
}

#[derive(Subcommand)]
pub enum ScreenshotSubcommands {
    Capture(crate::screenshot::capture::CaptureArgs),
}
