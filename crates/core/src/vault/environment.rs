//! Literal .env documents. Parsing never executes commands, expands variables or
//! consults process state. Export normalizes comments/layout, preserving values.
use crate::ProjectId;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fmt};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

pub const MAX_ENV_BYTES: usize = 1024 * 1024;
pub const MAX_ENV_VARIABLES: usize = 512;
pub const MAX_ENV_VALUE_BYTES: usize = 64 * 1024;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct EnvVariable {
    key: String,
    value: String,
}
impl fmt::Debug for EnvVariable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EnvVariable([redacted])")
    }
}
impl EnvVariable {
    pub fn new(key: &str, value: &str) -> Result<Self, EnvError> {
        if !valid_key(key) {
            return Err(EnvError::new(0, EnvProblem::InvalidKey));
        }
        if value.len() > MAX_ENV_VALUE_BYTES || value.contains('\0') {
            return Err(EnvError::new(0, EnvProblem::InvalidValue));
        }
        Ok(Self {
            key: key.to_owned(),
            value: value.to_owned(),
        })
    }
    pub fn key(&self) -> &str {
        &self.key
    }
    pub fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct EnvironmentSecrets {
    #[zeroize(skip)]
    project_id: Option<ProjectId>,
    environment: String,
    variables: Vec<EnvVariable>,
}
impl fmt::Debug for EnvironmentSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EnvironmentSecrets([redacted])")
    }
}
impl EnvironmentSecrets {
    pub fn new(
        project_id: Option<ProjectId>,
        environment: &str,
        variables: Vec<EnvVariable>,
    ) -> Result<Self, EnvError> {
        let document = Self {
            project_id,
            environment: environment.trim().to_owned(),
            variables,
        };
        document.validate()?;
        Ok(document)
    }
    pub fn variables(&self) -> &[EnvVariable] {
        &self.variables
    }
    pub fn project_id(&self) -> Option<ProjectId> {
        self.project_id
    }
    pub fn environment(&self) -> &str {
        &self.environment
    }
    pub fn validate(&self) -> Result<(), EnvError> {
        if self.environment.len() > 200
            || self.environment.contains(['\0', '\n', '\r'])
            || self.variables.len() > MAX_ENV_VARIABLES
        {
            return Err(EnvError::new(0, EnvProblem::TooLarge));
        }
        let mut seen = HashSet::new();
        let mut size = 0usize;
        for v in &self.variables {
            if !valid_key(&v.key) {
                return Err(EnvError::new(0, EnvProblem::InvalidKey));
            }
            if v.value.len() > MAX_ENV_VALUE_BYTES || v.value.contains('\0') {
                return Err(EnvError::new(0, EnvProblem::InvalidValue));
            }
            if !seen.insert(v.key.as_str()) {
                return Err(EnvError::new(0, EnvProblem::DuplicateKey));
            }
            // Measure the canonical output exactly; ordinary UTF-8 bytes do not expand.
            let escaped = v
                .value
                .bytes()
                .filter(|b| matches!(b, b'\\' | b'"' | b'\n' | b'\r' | b'\t' | b'$' | b'`'))
                .count();
            size = size.saturating_add(v.key.len() + v.value.len() + escaped + 4);
        }
        if size > MAX_ENV_BYTES {
            return Err(EnvError::new(0, EnvProblem::TooLarge));
        }
        Ok(())
    }
    pub fn parse(text: &str) -> Result<Self, EnvError> {
        if text.len() > MAX_ENV_BYTES || text.contains('\0') {
            return Err(EnvError::new(1, EnvProblem::TooLarge));
        }
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let lines: Vec<&str> = text.lines().collect();
        let mut variables = Vec::new();
        let mut seen = HashSet::new();
        let mut index = 0;
        while index < lines.len() {
            let line_number = index + 1;
            let mut line = lines[index].trim_start();
            index += 1;
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line
                .strip_prefix("export")
                .filter(|rest| rest.starts_with(char::is_whitespace))
            {
                line = rest.trim_start();
            }
            let (key, raw) = line
                .split_once('=')
                .ok_or(EnvError::new(line_number, EnvProblem::MissingEquals))?;
            let key = key.trim();
            if !valid_key(key) {
                return Err(EnvError::new(line_number, EnvProblem::InvalidKey));
            }
            if !seen.insert(key) {
                return Err(EnvError::new(line_number, EnvProblem::DuplicateKey));
            }
            let raw = raw.trim_start();
            let mut value = Zeroizing::new(String::new());
            if raw.starts_with(['\'', '"']) {
                let quote = raw
                    .chars()
                    .next()
                    .ok_or(EnvError::new(line_number, EnvProblem::UnclosedQuote))?;
                let mut segment = &raw[1..];
                let mut escaped = false;
                loop {
                    let mut closed = None;
                    for (offset, c) in segment.char_indices() {
                        if escaped {
                            match c {
                                'n' => value.push('\n'),
                                'r' => value.push('\r'),
                                't' => value.push('\t'),
                                '\\' | '"' | '$' | '`' => value.push(c),
                                _ => {
                                    value.push('\\');
                                    value.push(c);
                                }
                            }
                            escaped = false;
                        } else if c == quote {
                            closed = Some(offset + c.len_utf8());
                            break;
                        } else if c == '\\' && quote == '"' {
                            escaped = true;
                        } else {
                            value.push(c);
                        }
                        if value.len() > MAX_ENV_VALUE_BYTES {
                            return Err(EnvError::new(line_number, EnvProblem::TooLarge));
                        }
                    }
                    if let Some(end) = closed {
                        let trailing = segment[end..].trim_start();
                        if !trailing.is_empty() && !trailing.starts_with('#') {
                            return Err(EnvError::new(line_number, EnvProblem::TrailingText));
                        }
                        break;
                    }
                    if index >= lines.len() {
                        return Err(EnvError::new(line_number, EnvProblem::UnclosedQuote));
                    }
                    if escaped {
                        value.push('\\');
                        escaped = false;
                    }
                    value.push('\n');
                    segment = lines[index];
                    index += 1;
                }
            } else {
                let comment = raw
                    .char_indices()
                    .find_map(|(i, c)| {
                        (c == '#' && (i == 0 || raw[..i].ends_with(char::is_whitespace)))
                            .then_some(i)
                    })
                    .unwrap_or(raw.len());
                value.push_str(raw[..comment].trim_end());
            }
            variables.push(
                EnvVariable::new(key, &value).map_err(|e| EnvError::new(line_number, e.problem))?,
            );
            if variables.len() > MAX_ENV_VARIABLES {
                return Err(EnvError::new(line_number, EnvProblem::TooLarge));
            }
        }
        Self::new(None, "", variables)
    }
    /// Literal canonical form for our documented dialect. No interpolation.
    pub fn to_env(&self) -> Result<Zeroizing<String>, EnvError> {
        self.validate()?;
        let mut out = Zeroizing::new(String::new());
        for v in &self.variables {
            out.push_str(&v.key);
            out.push_str("=\"");
            for c in v.value.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    '$' => out.push_str("\\$"),
                    '`' => out.push_str("\\`"),
                    _ => out.push(c),
                }
            }
            out.push_str("\"\n");
        }
        Ok(out)
    }
}
fn valid_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    key.len() <= 256
        && bytes
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvProblem {
    InvalidKey,
    InvalidValue,
    DuplicateKey,
    MissingEquals,
    UnclosedQuote,
    TrailingText,
    TooLarge,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvError {
    pub line: usize,
    pub problem: EnvProblem,
}
impl EnvError {
    fn new(line: usize, problem: EnvProblem) -> Self {
        Self { line, problem }
    }
}
impl fmt::Display for EnvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(f, "Line {}: ", self.line)?;
        }
        f.write_str(match self.problem {
            EnvProblem::InvalidKey => "Use a variable name containing letters, digits and underscores, starting with a letter or underscore.",
            EnvProblem::InvalidValue => "A value is too large or contains an unsupported null character.",
            EnvProblem::DuplicateKey => "A variable name appears more than once. Keep one value before importing.",
            EnvProblem::MissingEquals => "Expected a KEY=VALUE assignment.",
            EnvProblem::UnclosedQuote => "A quoted value wasn't closed.",
            EnvProblem::TrailingText => "Unexpected text after a quoted value.",
            EnvProblem::TooLarge => "This environment set exceeds the import or variable limits.",
        })
    }
}
impl std::error::Error for EnvError {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn common_dotenv_values_are_literal_and_round_trip() {
        let doc = EnvironmentSecrets::parse("\u{feff}# comment\r\nexport EMPTY=\r\nRAW=a=b#c # comment\r\nQUOTED=\"  a # = b  \"\r\nKEY='line one\nline two'\nEXPAND=${HOME}\nCMD=$(whoami)\n").expect("parse");
        let values: Vec<_> = doc
            .variables()
            .iter()
            .map(|v| (v.key(), v.value()))
            .collect();
        assert_eq!(
            values,
            [
                ("EMPTY", ""),
                ("RAW", "a=b#c"),
                ("QUOTED", "  a # = b  "),
                ("KEY", "line one\nline two"),
                ("EXPAND", "${HOME}"),
                ("CMD", "$(whoami)")
            ]
        );
        assert_eq!(
            EnvironmentSecrets::parse(&doc.to_env().expect("export")).expect("again"),
            doc
        );
        let special = EnvVariable::new("SPECIAL", "'\"\\\n\r\t$`whoami`💚  ").expect("variable");
        let d = EnvironmentSecrets::new(None, "", vec![special]).expect("doc");
        assert_eq!(
            EnvironmentSecrets::parse(&d.to_env().expect("export")).expect("again"),
            d
        );
    }
    #[test]
    fn invalid_imports_fail_without_echoing_secrets() {
        for source in [
            "A=topsecret\nA=other",
            "BAD-KEY=topsecret",
            "A=\"topsecret",
            "A=\"topsecret\" trailing",
            "topsecret",
        ] {
            let err = EnvironmentSecrets::parse(source).expect_err("invalid");
            assert!(!err.to_string().contains("topsecret"));
        }
        let source = (0..MAX_ENV_VARIABLES + 1)
            .map(|i| format!("K{i}=x\n"))
            .collect::<String>();
        assert!(EnvironmentSecrets::parse(&source).is_err());
        assert!(
            EnvironmentSecrets::parse(&format!("A={}", "x".repeat(MAX_ENV_VALUE_BYTES + 1)))
                .is_err()
        );
    }
    #[test]
    fn more_than_thirty_variables_survive_serialization() {
        let source = (0..200)
            .map(|i| format!("K{i}=value {i}\n"))
            .collect::<String>();
        let d = EnvironmentSecrets::parse(&source).expect("parse");
        let bytes = serde_json::to_vec(&d).expect("json");
        let restored: EnvironmentSecrets = serde_json::from_slice(&bytes).expect("read");
        assert_eq!(restored.variables().len(), 200);
        assert_eq!(format!("{restored:?}"), "EnvironmentSecrets([redacted])");
    }
    #[test]
    fn limits_and_untrusted_json_are_validated_without_truncation() {
        let rows = (0..MAX_ENV_VARIABLES)
            .map(|i| EnvVariable::new(&format!("K{i}"), "x").expect("variable"))
            .collect();
        let doc = EnvironmentSecrets::new(None, "Development", rows).expect("maximum count");
        assert_eq!(doc.variables().len(), MAX_ENV_VARIABLES);
        let json = serde_json::json!({"project_id":null,"environment":"Development","variables":[{"key":"BAD-KEY","value":"very-secret"}]});
        let bad: EnvironmentSecrets = serde_json::from_value(json).expect("decode");
        assert!(bad.validate().is_err());
        assert!(
            !bad.validate()
                .expect_err("invalid")
                .to_string()
                .contains("very-secret")
        );
        assert!(EnvironmentSecrets::parse(&"#".repeat(MAX_ENV_BYTES + 1)).is_err());
        assert!(EnvironmentSecrets::parse("KEY=a\0b").is_err());
        assert!(EnvironmentSecrets::new(None, "bad\nlabel", vec![]).is_err());
        assert!(
            EnvironmentSecrets::new(
                None,
                "Development",
                vec![
                    EnvVariable::new("A", "a").expect("a"),
                    EnvVariable::new("A", "b").expect("b")
                ]
            )
            .is_err()
        );
    }

    #[test]
    fn limits_measure_actual_export_bytes_including_escaping_and_utf8() {
        let value = "x".repeat(MAX_ENV_VALUE_BYTES);
        let source = (0..12)
            .map(|i| format!("K{i}={value}\n"))
            .collect::<String>();
        assert!(source.len() > MAX_ENV_BYTES / 2 && source.len() < MAX_ENV_BYTES);
        let doc = EnvironmentSecrets::parse(&source).expect("valid file above 512 KiB");
        let exported = doc.to_env().expect("export");
        assert_eq!(
            EnvironmentSecrets::parse(&exported).expect("round trip"),
            doc
        );
        let unicode = EnvironmentSecrets::new(
            None,
            "",
            vec![EnvVariable::new("KEY", "é💚\\\n\t\r\"$`").expect("value")],
        )
        .expect("doc");
        assert!(unicode.to_env().expect("export").len() < 100);
        let escaped = (0..9)
            .map(|i| {
                EnvVariable::new(&format!("K{i}"), &"\\".repeat(MAX_ENV_VALUE_BYTES)).expect("row")
            })
            .collect();
        assert!(
            EnvironmentSecrets::new(None, "", escaped).is_err(),
            "actual canonical export exceeds 1 MiB"
        );
    }
}
