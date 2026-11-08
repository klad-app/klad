pub const SUPPORT: &str = "hello@klad.studio";
pub const SITE: &str = "https://klad.studio";
pub const ISSUES: &str = "https://github.com/klad-app/klad/issues";

pub fn mailto(subject: &str, body: &str) -> String {
    format!(
        "mailto:{SUPPORT}?subject={}&body={}",
        encode(subject),
        encode(&body.replace("\r\n", "\n").replace('\n', "\r\n"))
    )
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mail_to_support_keeps_its_lines_and_spaces() {
        assert_eq!(
            mailto("klad 1.0: help", "line one\nline 2 & more"),
            "mailto:hello@klad.studio?subject=klad%201.0%3A%20help&body=line%20one%0D%0Aline%202%20%26%20more"
        );
    }
}
