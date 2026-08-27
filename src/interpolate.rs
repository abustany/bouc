use std::collections::HashMap;

use anyhow::{Context, Result, bail};

pub fn interpolate(fmt: &str, data: &HashMap<&str, &str>) -> Result<String> {
    let mut out = String::with_capacity(fmt.len());
    let mut rest = fmt;

    while let Some(idx) = rest.find(['{', '}']) {
        let (before, tail) = rest.split_at(idx);
        out.push_str(before);

        let delim = tail.as_bytes()[0];

        if tail.as_bytes().get(1) == Some(&delim) {
            out.push(char::from(delim));
            rest = &tail[2..];
            continue;
        }

        if delim == b'}' {
            bail!("Unmatched }}");
        }

        let end = tail.find('}').context("Unmatched {")?;
        let name = &tail[1..end];
        out.push_str(
            data.get(name)
                .with_context(|| format!("Undefined variable: {name}"))?,
        );
        rest = &tail[end + 1..];
    }

    out.push_str(rest);

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpolate() {
        assert_eq!(interpolate("", &HashMap::new()).unwrap(), "");
        assert_eq!(
            interpolate("Hello {name}", &HashMap::from([("name", "Alice")])).unwrap(),
            "Hello Alice"
        );
        assert_eq!(
            interpolate("Hello {{name}}", &HashMap::from([("name", "Alice")])).unwrap(),
            "Hello {name}"
        );
        assert_eq!(
            interpolate("Hello {name}", &HashMap::from([("other", "Alice")]))
                .unwrap_err()
                .to_string(),
            "Undefined variable: name"
        );
    }
}
