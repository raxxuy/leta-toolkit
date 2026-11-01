use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::Args;
use serde::Deserialize;

use super::{colors, templates};

#[derive(Args, Debug)]
pub struct PaletteArgs {
    #[arg(long)]
    image: PathBuf,
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    templates: BTreeMap<PathBuf, PathBuf>,
}

pub fn run(args: PaletteArgs) -> Result<()> {
    let colors = colors::extract(&args.image)?;
    match args.config {
        Some(config_path) => render_config(&config_path, &colors),
        None => {
            for (name, hex) in colors {
                println!("{name}={hex}");
            }
            Ok(())
        }
    }
}

fn load_config(path: &Path) -> Result<Config> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read config {}", path.display()))?;
    toml::from_str(&contents)
        .with_context(|| format!("failed to parse config {}", path.display()))
}

fn render_template(input: &Path, colors: &[(String, String)]) -> Result<String> {
    let template = fs::read_to_string(input)
        .with_context(|| format!("failed to read template {}", input.display()))?;
    templates::render(&template, colors)
        .with_context(|| format!("failed to render template {}", input.display()))
}

fn write_output(output: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }
    fs::write(output, contents)
        .with_context(|| format!("failed to write output {}", output.display()))
}

fn render_config(config_path: &Path, colors: &[(String, String)]) -> Result<()> {
    let config = load_config(config_path)?;
    let base = config_path.parent().unwrap_or(Path::new(""));

    // Render everything first so a bad template leaves all outputs untouched.
    let rendered = config
        .templates
        .into_iter()
        .map(|(input, output)| {
            Ok((base.join(output), render_template(&base.join(input), colors)?))
        })
        .collect::<Result<Vec<_>>>()?;

    rendered
        .iter()
        .try_for_each(|(output, contents)| write_output(output, contents))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_relative_paths_and_does_not_write_on_template_error() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("leta-palette-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let image_path = dir.join("image.png");
        ::image::RgbaImage::from_pixel(2, 2, ::image::Rgba([255, 0, 0, 255]))
            .save(&image_path)
            .unwrap();
        fs::write(
            dir.join("palette.toml"),
            "[templates]\n\"input\" = \"out/colors\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("input"),
            "{{primary}} {{#colors}}{{name}}={{hex}};{{/colors}}",
        )
        .unwrap();
        run(PaletteArgs {
            image: image_path,
            config: Some(dir.join("palette.toml")),
        })
        .unwrap();
        let output = fs::read_to_string(dir.join("out/colors")).unwrap();
        assert!(output.contains("primary=#"));
        fs::write(dir.join("input"), "{{unknown}}").unwrap();
        assert!(
            render_config(
                &dir.join("palette.toml"),
                &[("primary".into(), "#fff".into())]
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(dir.join("out/colors")).unwrap(), output);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn config_parses_template_paths_and_rejects_unknown_fields() {
        let config: Config = toml::from_str("[templates]\n\"in.scss\" = \"out.scss\"\n").unwrap();
        assert_eq!(
            config.templates.get(Path::new("in.scss")),
            Some(&PathBuf::from("out.scss"))
        );
        assert!(toml::from_str::<Config>("mode = 'dark'\n[templates]\n").is_err());
    }
}
