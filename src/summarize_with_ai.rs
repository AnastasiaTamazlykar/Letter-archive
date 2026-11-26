use serde::Deserialize;
use serde_json::{Value, json};
use std::error::Error;

#[derive(Debug, Deserialize)]
pub struct LLMAnswer {
    /// The folder the LLM chose for the letter, like "Gesundheit/teeth"; `None` if none
    /// fits. Always one of the folders it was offered (see `chosen_folder`).
    pub folder: Option<String>,
    pub summary: String,
    pub letter_date: String,
    /// A short file name for the letter, as the LLM suggests it; may contain
    /// characters a file name can't have, so clean it before use (`pdf_name`).
    pub file_name: String,
}

#[derive(Debug, Deserialize)]
struct Parsed {
    #[serde(default)]
    pub folder: String,
    pub summary: String,
    pub letter_date: String,
    /// `serde(default)`: if the LLM leaves the key out, this is "" instead of an error.
    #[serde(default)]
    pub file_name: String,
}

const SERVER_URL: &str = "http://localhost:8080/v1/chat/completions";
const PROMPT: &str = "Please summarize this letter in several sentences in german.
Respond with only a JSON object, no Markdown, with 4 keys: 'folder', 'summary', 'letter_date' and 'file_name'.
 'file_name' is a short name for the letter's file, without ending and spaces, must summarize a letter intention und uniqueness in 3 words, shortenings allowed.
 For 'folder' choose the most suitable folder for this letter from this list, or \"\" if none fits:
 {FOLDERS}. letter date can be empty if not found, otherwise return in iso format. For summary nenne Absender, Anliegen und Fristen. Letter text : ";

/// Asks the local LLM to sum up the letter, name it, and pick one of `folders` for it.
pub fn ask_model(document_text: &str, folders: &[String]) -> Result<LLMAnswer, Box<dyn Error>> {
    // As a JSON list, like ["Gesundheit","Gesundheit/teeth"]: clear even if a name has a comma.
    let folder_list = serde_json::to_string(folders)?;
    let prompt = PROMPT.replacen("{FOLDERS}", &folder_list, 1);
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
        file_name: answer.file_name,
        letter_date: answer.letter_date,
        summary: answer.summary,
        folder: chosen_folder(&answer.folder, folders),
    })
}

/// The folder the LLM answered, if it is one of `folders` (a `/` at either end is ignored).
/// Anything else, like "" or a folder the LLM made up, gives `None`; so a file can never
/// be put outside the output folder, for example with "../".
fn chosen_folder(answer: &str, folders: &[String]) -> Option<String> {
    let answer = answer.trim().trim_matches('/');
    // `cloned` turns the borrowed `&String` found in the list into an owned `String`.
    folders.iter().find(|folder| folder.as_str() == answer).cloned()
}

/// Folders the tests offer to the LLM.
#[cfg(test)]
fn test_folders() -> Vec<String> {
    ["Finanzamt", "Gesundheit", "Gesundheit/teeth", "Rundfunkbeitrag"]
        .map(String::from)
        .to_vec()
}

#[test]
fn test_chosen_folder_accepts_only_offered_folders() {
    let folders = test_folders();
    assert_eq!(chosen_folder("Gesundheit/teeth", &folders).as_deref(), Some("Gesundheit/teeth"));
    assert_eq!(chosen_folder(" /Finanzamt/ ", &folders).as_deref(), Some("Finanzamt"));
    assert_eq!(chosen_folder("Steuer", &folders), None);
    assert_eq!(chosen_folder("../Finanzamt", &folders), None);
}

#[test]
fn test_chosen_folder_empty_answer_is_none() {
    assert_eq!(chosen_folder("", &test_folders()), None);
    assert_eq!(chosen_folder("Finanzamt", &[]), None);
}

#[test]
#[ignore = "stop model first"]
fn test_server_is_down_negative() {
    let resp = ask_model("asdasdasd", &[]);
    let err = resp.unwrap_err();
    assert!(err.to_string().contains("Connection refused"));
}

#[test]
fn test_letter_is_nonsense_negative() {
    let resp = ask_model("server is down", &test_folders()).unwrap();
    assert!(resp.summary.starts_with("Der Abs"));
    assert_eq!(resp.folder, None);
}

#[test]
fn test_letter_ard_positive() {
    const LETTER: &str = "ARDO 2DF
Deutschlandradio
BEITRAGSSERVICE
Frau
Anastasiia Tol
Postanschrift
ARD ZDF Deutschlandradio
Beitragsservice, 50656 Köln
Datum 25.11.2025
Beitragsnummer 265 853 781
Sehr geehrte Frau Tol,
Ihre Rundfunkbeitrage sind am 15.12.2025 fällig.
Bitte zahlen Sie den Betrag von 55,08 EUR. ";
    let resp = ask_model(LETTER, &test_folders()).unwrap();
    assert_eq!(resp.letter_date, "2025-11-25");
    assert!(resp.summary.contains("Tol"));
    assert_eq!(resp.folder.as_deref(), Some("Rundfunkbeitrag"));
}

#[test]
fn test_fancy_date() {
    const LETTER: &str = "F
Herrr Frizt
mûnchen, 10. November 2026
misik invitation ololo";
    let resp = ask_model(LETTER, &[]).unwrap();
    assert_eq!(resp.letter_date, "2026-11-10");
}


fn verify_date(date: &str)->Option<String>{
    use jiff::civil::Date;
    date.parse::<Date>().ok().map(|d| d.to_string())
}
#[test]
fn test_jiff_date_transforms_test(){
    assert_eq!(verify_date("2025-02-01"), Some("2025-02-01".to_string()));
    assert_eq!(verify_date("20df25-02-01"), None);

}