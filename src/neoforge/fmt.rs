use std::path::Path;

use anyhow::bail;
use creeper_maven_coord::MavenCoord;

pub fn maven_coord_format(s: &str, root: impl AsRef<Path>) -> anyhow::Result<String> {
    let root = root.as_ref();
    let mut result = String::new();

    let mut in_var = false;
    let mut var_start = 0;
    let mut var = String::new();

    for (pos, ch) in s.char_indices() {
        match ch {
            '[' => {
                if in_var {
                    bail!("{s}: nested bracket at {pos}");
                }

                in_var = true;
                var_start = pos;
                var.clear();
            }

            ']' => {
                if !in_var {
                    bail!("{s}: unexpected closing bracket at {pos}");
                }

                let coord = var.parse::<MavenCoord>()?;

                let path = root.join(coord.path());

                result.push_str(&path.display().to_string());

                in_var = false;
            }

            c => {
                if in_var {
                    var.push(c);
                } else {
                    result.push(c);
                }
            }
        }
    }

    if in_var {
        bail!("{s}: unclosed bracket at {var_start}");
    }

    Ok(result)
}
