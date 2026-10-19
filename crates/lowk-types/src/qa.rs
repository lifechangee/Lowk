//! Q&A link DTOs («Вопрос -> подсказка ИИ -> Мой ответ», ARCHITECTURE 5.5).
use crate::{MonoNs, QaId};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CloseReason {
    NextAsk,
    SystemQuestion,
    Timeout,
    SessionEnd,
}

/// One question/hint/answer triple.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct QaRecord {
    pub qa_id: QaId,
    pub question: String,
    pub hint: String,
    pub my_answer: String,
    pub armed_at: MonoNs,
    /// Event time of the first collected Me segment.
    pub first_at: Option<MonoNs>,
    /// `None` while the link is still collecting.
    pub closed: Option<CloseReason>,
}

/// `qa://my-answer`: the «Мой ответ» text of a link changed or the link closed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct QaUpdate {
    pub qa_id: QaId,
    pub text: String,
    pub closed: Option<CloseReason>,
}
