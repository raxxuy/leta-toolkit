use clap::Args;
use image::{imageops::FilterType, GenericImageView};
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct CoverArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    width: u32,
    #[arg(long)]
    height: u32,
}

pub fn run(args: CoverArgs) -> anyhow::Result<()> {
    let img = image::open(&args.input)?;
    let (src_w, src_h) = img.dimensions();

    let scale = (args.width as f64 / src_w as f64)
        .max(args.height as f64 / src_h as f64);

    let scaled_w = (src_w as f64 * scale).round() as u32;
    let scaled_h = (src_h as f64 * scale).round() as u32;

    let resized = img.resize_exact(scaled_w, scaled_h, FilterType::Triangle);

    let offset_x = (scaled_w - args.width) / 2;
    let offset_y = (scaled_h - args.height) / 2;

    let cropped = resized.crop_imm(offset_x, offset_y, args.width, args.height);

    cropped.save(&args.output)?;
    Ok(())
}
