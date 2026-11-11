use crate::pdf_crud::LetterCategory;
use serde::Deserialize;
use serde_json::{Value, json};
use std::error::Error;

#[derive(Debug, Deserialize)]
pub struct LLMAnswer {
    pub category: LetterCategory,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
struct Parsed {
    pub category: String,
    pub summary: String,
}

const SERVER_URL: &str = "http://localhost:8080/v1/chat/completions";
const PROMPT: &str = "Please summarize this letter in several sentences in german. Respond with only a JSON object, no Markdown, with 2 keys: 'category' and 'summary'.
 For category choose most relevant among:
 {CATEGORIES}. For summary nenne Absender, Anliegen und Fristen. Letter text : ";

pub fn ask_model(document_text: &str) -> Result<LLMAnswer, Box<dyn Error>> {
    let list_cat = LetterCategory::ALL
        .iter()
        .map(|x| x.name())
        .collect::<Vec<_>>()
        .join(", ");
    let prompt = PROMPT.replacen("{CATEGORIES}", &list_cat, 1);
    let body = json!({"messages":[{"content": format!("{prompt}{document_text}"), "role": "user"}],
        "max_tokens": 600,
        "chat_template_kwargs": {"enable_thinking": false}
        }
    );
    let response: Value = ureq::post(SERVER_URL)
        .send_json(&body)?
        .body_mut()
        .read_json()?;
    let answer: Parsed = serde_json::from_str(
        &response["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("server response has no content")?,
    )?;
    Ok(LLMAnswer {
        summary: answer.summary,
        category: LetterCategory::from_name(answer.category.as_str())
            .unwrap_or(LetterCategory::Sonstiges),
    })
}

#[test]
#[ignore = "stop model first"]
fn test_server_is_down_negative() {
    let resp = ask_model("asdasdasd");
    let err = resp.unwrap_err();
    assert!(err.to_string().contains("Connection refused"));
}

#[test]
fn test_letter_is_nonsense_negative() {
    let resp = ask_model("server is down").unwrap();
    assert!(resp.summary.starts_with("Der Abs"));
    assert!(resp.category == LetterCategory::Sonstiges);
}

#[test]
fn test_letter_ard_positive() {
    const LETTER: &str = "ARDO 2DF
Deutschlandradio
BEITRAGSSERVICE
Frau
Anastasiia Tol
Sehr geehrte Frau Tol,
Ihre Rundfunkbeitrage sind am 15.12.2025 fällig.
Bitte zahlen Sie den Betrag von 55,08 EUR. ";
    let resp = ask_model(LETTER).unwrap();
    assert!(resp.summary.contains("Tol"));
    assert!(resp.category == LetterCategory::Rundfunkbeitrag);
}
