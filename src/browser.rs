use crate::analyzer::SemanticAnalyzer;
use crate::builder::build_ast;
use crate::evaluator::Session;
use crate::parser::parse_vex;

fn quote_json(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

/// Interpret a Vex source snippet and return a compact JSON response for the
/// browser adapter. This intentionally exposes no filesystem or process APIs.
pub fn run_json(source: &str) -> String {
    let parsed = match parse_vex(source) {
        Ok(pairs) => pairs,
        Err(error) => return error_json(&error.to_string()),
    };
    let statements = match build_ast(parsed) {
        Ok(statements) => statements,
        Err(error) => return error_json(&error.to_string()),
    };
    let mut analyzer = SemanticAnalyzer::new();
    if let Err(error) = analyzer.analyze(&statements) {
        return error_json(&error);
    }

    let mut session = Session::new();
    let (evaluated, stdout, output_truncated) = session.eval_capturing_output(&statements);
    if output_truncated {
        return format!(
            "{{\"ok\":false,\"stdout\":{},\"error\":{}}}",
            quote_json(&stdout),
            quote_json("Output limit reached (12 KB).")
        );
    }
    match evaluated {
        Ok(value) => format!(
            "{{\"ok\":true,\"stdout\":{},\"value\":{}}}",
            quote_json(&stdout),
            quote_json(&format!("{value:?}"))
        ),
        Err(error) => format!(
            "{{\"ok\":false,\"stdout\":{},\"error\":{}}}",
            quote_json(&stdout),
            quote_json(&error.to_string())
        ),
    }
}

fn error_json(error: &str) -> String {
    format!(
        "{{\"ok\":false,\"stdout\":\"\",\"error\":{}}}",
        quote_json(error)
    )
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn run_vex(source: &str) -> String {
    run_json(source)
}

#[cfg(test)]
mod tests {
    use super::run_json;

    #[test]
    fn run_json_captures_output_and_value() {
        let result = run_json("let answer: i32 = 6 * 7; println(\"signal\"); answer;");
        assert!(result.contains("\"ok\":true"));
        assert!(result.contains("\"stdout\":\"signal\\n\""));
        assert!(result.contains("\"value\":\"Int(42)\""));
    }

    #[test]
    fn run_json_returns_errors_as_data_and_keeps_prior_output() {
        let result = run_json("println(\"before\"); 1 / 0;");
        assert!(result.contains("\"ok\":false"));
        assert!(result.contains("\"stdout\":\"before\\n\""));
        assert!(result.contains("division by zero"));
    }

    #[test]
    fn run_json_caps_output_during_interpretation() {
        let result = run_json(
            "let count: i32 = 0; while count < 2000 { println(\"12345678\"); count = count + 1; } count;",
        );
        assert!(result.contains("\"ok\":false"));
        assert!(result.contains("Output limit reached (12 KB)."));
        let stdout = result.split("\"stdout\":\"").nth(1).unwrap();
        // Newlines are JSON-escaped, so the response can be up to roughly
        // twice the captured bytes while still remaining within the 12 KB cap.
        assert!(stdout.len() < 24_100);
    }
}
