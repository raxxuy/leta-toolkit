use anyhow::{Result, anyhow, bail};

type Color = (String, String);

#[derive(Debug)]
enum Part<'a> {
    Text(&'a str),
    Value(&'a str),
    Name,
    Hex,
    Colors(Vec<Part<'a>>),
}

fn parse<'a>(template: &'a str, colors: &'a [Color]) -> Result<Vec<Part<'a>>> {
    let mut parts = Vec::new();
    let mut loop_body: Option<Vec<Part<'a>>> = None;
    let mut rest = template;

    while !rest.is_empty() {
        let open = rest.find("{{");
        if rest
            .find("}}")
            .is_some_and(|close| open.is_none_or(|open| close < open))
        {
            bail!("unexpected closing delimiter");
        }

        let target = loop_body.as_mut().unwrap_or(&mut parts);
        let Some(start) = open else {
            target.push(Part::Text(rest));
            break;
        };
        target.push(Part::Text(&rest[..start]));

        let after_open = &rest[start + 2..];
        let Some(end) = after_open.find("}}") else {
            bail!("unclosed template tag");
        };
        let tag = after_open[..end].trim();
        if tag.is_empty() || tag.contains(['{', '}']) {
            bail!("malformed template tag: {tag:?}");
        }
        rest = &after_open[end + 2..];

        match tag {
            "#colors" => {
                if loop_body.is_some() {
                    bail!("nested colors loops are not supported");
                }
                loop_body = Some(Vec::new());
            }
            "/colors" => {
                let Some(body) = loop_body.take() else {
                    bail!("unexpected colors loop closing tag");
                };
                parts.push(Part::Colors(body));
            }
            _ => {
                let part = parse_variable(tag, loop_body.is_some(), colors)?;
                loop_body.as_mut().unwrap_or(&mut parts).push(part);
            }
        }
    }

    if loop_body.is_some() {
        bail!("unclosed colors loop");
    }
    Ok(parts)
}

fn parse_variable<'a>(tag: &str, in_loop: bool, colors: &'a [Color]) -> Result<Part<'a>> {
    match tag {
        "name" if in_loop => Ok(Part::Name),
        "hex" if in_loop => Ok(Part::Hex),
        _ if tag.starts_with(['#', '/']) => bail!("unknown template control tag: {tag}"),
        _ => colors
            .iter()
            .find(|(name, _)| name == tag)
            .map(|(_, hex)| Part::Value(hex))
            .ok_or_else(|| anyhow!("unknown template variable: {tag}")),
    }
}

fn append(parts: &[Part<'_>], colors: &[Color], entry: Option<&Color>, output: &mut String) {
    for part in parts {
        match (part, entry) {
            (Part::Text(text) | Part::Value(text), _) => output.push_str(text),
            (Part::Name, Some((name, _))) => output.push_str(name),
            (Part::Hex, Some((_, hex))) => output.push_str(hex),
            (Part::Colors(body), _) => {
                for color in colors {
                    append(body, colors, Some(color), output);
                }
            }
            (Part::Name | Part::Hex, None) => {
                unreachable!("loop variables are only parsed inside loops")
            }
        }
    }
}

/// Render palette variables and `#colors` loops without escaping or re-interpolation.
///
/// Inside a loop, `name` and `hex` refer to the current entry; other variables
/// refer to palette entries. If names are duplicated, variables use the first.
pub fn render(template: &str, colors: &[(String, String)]) -> Result<String> {
    let parts = parse(template, colors)?;
    let mut output = String::new();
    append(&parts, colors, None, &mut output);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::render;

    fn colors() -> Vec<(String, String)> {
        vec![
            ("primary".into(), "#123456".into()),
            ("accent".into(), "#abcdef".into()),
        ]
    }

    #[test]
    fn substitutes_and_repeats_without_changing_format() {
        let template = "{\n  primary: {{ primary }},\n{{ #colors }}  {{ name }}={{ hex }} / {{primary}}\n{{ /colors }}}";
        assert_eq!(
            render(template, &colors()).unwrap(),
            "{\n  primary: #123456,\n  primary=#123456 / #123456\n  accent=#abcdef / #123456\n}"
        );
    }

    #[test]
    fn empty_palette_and_multiple_loops() {
        assert_eq!(
            render("a{{#colors}}{{name}}={{hex}}{{/colors}}b", &[]).unwrap(),
            "ab"
        );
        assert_eq!(
            render(
                "{{#colors}}{{name}}{{/colors}}|{{#colors}}{{hex}}{{/colors}}",
                &colors()
            )
            .unwrap(),
            "primaryaccent|#123456#abcdef"
        );
    }

    #[test]
    fn values_are_not_interpolated() {
        let colors = vec![
            ("primary".into(), "{{missing}}".into()),
            ("{{primary}}".into(), "{{#colors}}".into()),
        ];
        assert_eq!(
            render(
                "{{primary}}|{{#colors}}{{name}}={{hex}};{{/colors}}",
                &colors
            )
            .unwrap(),
            "{{missing}}|primary={{missing}};{{primary}}={{#colors}};"
        );
    }

    #[test]
    fn reports_unknown_variables_even_in_empty_loops() {
        for template in [
            "{{missing}}",
            "{{name}}",
            "{{hex}}",
            "{{#colors}}{{missing}}{{/colors}}",
        ] {
            assert!(render(template, &[]).is_err(), "{template}");
        }
    }

    #[test]
    fn rejects_invalid_loop_structure_and_delimiters() {
        for template in [
            "{{#colors}}",
            "{{/colors}}",
            "{{#colors}}{{#colors}}{{/colors}}{{/colors}}",
            "{{primary",
            "primary}}",
            "{{}}",
            "{{  }}",
            "{{{primary}}}",
            "{{primary{{accent}}",
            "{{#unknown}}",
            "{{/unknown}}",
        ] {
            assert!(render(template, &colors()).is_err(), "{template}");
        }
    }

    #[test]
    fn reports_specific_parse_errors() {
        for (template, expected) in [
            ("}}", "unexpected closing delimiter"),
            ("{{primary", "unclosed template tag"),
            ("{{ }}", "malformed template tag: \"\""),
            (
                "{{#colors}}{{#colors}}",
                "nested colors loops are not supported",
            ),
            ("{{/colors}}", "unexpected colors loop closing tag"),
            ("{{#unknown}}", "unknown template control tag: #unknown"),
            ("{{missing}}", "unknown template variable: missing"),
            ("{{#colors}}", "unclosed colors loop"),
        ] {
            assert_eq!(
                render(template, &colors()).unwrap_err().to_string(),
                expected
            );
        }
    }

    #[test]
    fn duplicate_names_use_first_value() {
        let colors = vec![("a".into(), "first".into()), ("a".into(), "second".into())];
        assert_eq!(
            render("{{a}} {{#colors}}{{a}} {{/colors}}", &colors).unwrap(),
            "first first first "
        );
    }

    #[test]
    fn loop_variables_shadow_palette_entries() {
        let colors = vec![
            ("name".into(), "outside".into()),
            ("hex".into(), "value".into()),
        ];
        assert_eq!(
            render(
                "{{name}} {{hex}} {{#colors}}{{name}}={{hex}};{{/colors}}",
                &colors
            )
            .unwrap(),
            "outside value name=outside;hex=value;"
        );
    }
}
