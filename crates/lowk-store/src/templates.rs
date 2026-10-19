//! Prompt templates (architecture 5.4). Stored in `settings.toml`
//! (`[[templates]]`); this is the editing/lookup layer.
use lowk_types::*;

pub const TEMPLATE_SUMMARY: &str = "summary";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateSet(Vec<Template>);

/// «Резюме»: used by `llm_summarize`; not offered in the overlay chip by default.
pub fn summary_template() -> Template {
    Template {
        id: TEMPLATE_SUMMARY.into(),
        names: LocalizedName { ru: "Резюме".into(), en: "Summary".into() },
        instruction: "Сделай краткое резюме созвона: ключевые темы, решения, договорённости и следующие шаги. Отвечай на языке разговора.".into(),
        needs_my_answer: false,
        vision: false,
        reasoning: false,
    }
}

impl TemplateSet {
    /// «Кратко», «Дополни мой ответ», «Резюме».
    pub fn defaults() -> Self {
        let mut v = default_templates();
        v.push(summary_template());
        Self(v)
    }

    pub fn from_settings(s: &Settings) -> Self {
        Self(s.templates.clone())
    }

    pub fn into_vec(self) -> Vec<Template> {
        self.0
    }

    pub fn all(&self) -> &[Template] {
        &self.0
    }

    pub fn get(&self, id: &str) -> Option<&Template> {
        self.0.iter().find(|t| t.id == id)
    }

    /// Template for a folder: its `default_template` when it exists, else the
    /// global default (`llm.default_template`), else `brief`.
    pub fn resolve<'a>(&'a self, folder_default: Option<&str>, global_default: &str) -> Option<&'a Template> {
        folder_default
            .and_then(|i| self.get(i))
            .or_else(|| self.get(global_default))
            .or_else(|| self.get(TEMPLATE_BRIEF))
            .or_else(|| self.0.first())
    }

    /// Insert or replace by id.
    pub fn upsert(&mut self, t: Template) -> Result<()> {
        if t.id.trim().is_empty() || t.instruction.trim().is_empty() {
            return Err(AppError::new(ErrorCode::InvalidArgument).with_detail("template needs id and text"));
        }
        match self.0.iter_mut().find(|x| x.id == t.id) {
            Some(x) => *x = t,
            None => self.0.push(t),
        }
        Ok(())
    }

    /// Remove a user template. Built-ins cannot be removed (edit them instead).
    pub fn remove(&mut self, id: &str) -> Result<()> {
        if Self::defaults().get(id).is_some() {
            return Err(AppError::new(ErrorCode::InvalidArgument).with_detail("built-in template"));
        }
        self.0.retain(|t| t.id != id);
        Ok(())
    }

    /// Add built-ins that are missing (e.g. after an upgrade); user edits stay.
    pub fn ensure_builtin(&mut self) {
        for d in Self::defaults().0 {
            if self.get(&d.id).is_none() {
                self.0.push(d);
            }
        }
    }

    /// Restore one built-in to its shipped text.
    pub fn reset(&mut self, id: &str) {
        if let Some(d) = Self::defaults().get(id).cloned() {
            let _ = self.upsert(d);
        }
    }

    pub fn to_patch(&self) -> SettingsPatch {
        SettingsPatch(serde_json::json!({ "templates": self.0 }))
    }
}
