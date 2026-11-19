//! Balasan XML GameStream: satu elemen `<root status_code="...">` berisi elemen teks sederhana.

use std::fmt::Display;

pub struct Xml {
    status: u16,
    message: Option<String>,
    body: String,
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

impl Xml {
    pub fn ok() -> Xml {
        Xml { status: 200, message: None, body: String::new() }
    }

    pub fn error(status: u16, message: &str) -> Xml {
        Xml { status, message: Some(message.to_string()), body: String::new() }
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    /// Tambah `<name>value</name>`. Nama elemen selalu konstanta di kode, hanya nilainya yang di-escape.
    pub fn field(mut self, name: &str, value: impl Display) -> Xml {
        self.body.push_str(&format!("<{name}>{}</{name}>", escape(&value.to_string())));
        self
    }

    /// Tambah elemen bersarang, mis. `<App>...</App>`.
    pub fn child(mut self, name: &str, fields: &[(&str, String)]) -> Xml {
        self.body.push_str(&format!("<{name}>"));
        for (field, value) in fields {
            self.body.push_str(&format!("<{field}>{}</{field}>", escape(value)));
        }
        self.body.push_str(&format!("</{name}>"));
        self
    }

    pub fn render(&self) -> String {
        let message = self.message.as_deref().map(|m| format!(" status_message=\"{}\"", escape(m))).unwrap_or_default();
        format!("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<root status_code=\"{}\"{message}>{}</root>", self.status, self.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_fields_and_escapes_values() {
        let xml = Xml::ok().field("hostname", "PC <A&B>").field("paired", 1).render();
        assert_eq!(xml, "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<root status_code=\"200\"><hostname>PC &lt;A&amp;B&gt;</hostname><paired>1</paired></root>");
    }

    #[test]
    fn error_carries_message_attribute() {
        let xml = Xml::error(400, "salah \"x\"").field("paired", 0).render();
        assert!(xml.contains("<root status_code=\"400\" status_message=\"salah &quot;x&quot;\"><paired>0</paired></root>"));
    }
}
