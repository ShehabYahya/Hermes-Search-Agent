use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LightSearchArgs {
    /// One narrow factual question to resolve. State what must be known; do not write a search-engine query.
    pub question: String,

    /// Context needed to disambiguate the question, such as the project, version, environment, or referent.
    #[serde(default)]
    pub context: Option<String>,

    /// Relevant date/freshness window, for example "current upstream" or "last 30 days".
    #[serde(default)]
    pub time_scope: Option<String>,

    /// Source requirements or preferences, for example "official documentation first".
    #[serde(default)]
    pub source_constraints: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MediumResearchArgs {
    /// The outcome the research must establish. Describe the decision/question, not the search process.
    pub objective: String,

    /// Background that materially affects how the objective should be interpreted.
    #[serde(default)]
    pub context: Option<String>,

    /// Questions that must be answered. These are mandatory coverage, not an exhaustive research plan.
    #[serde(default)]
    pub must_answer: Vec<String>,

    /// Boundaries such as platform, version, geography, timeframe, population, or environment.
    #[serde(default)]
    pub scope: Option<String>,

    /// Requirements for source type or quality. The researcher still chooses search queries and sources.
    #[serde(default)]
    pub source_constraints: Option<String>,

    /// Requested shape of the final handoff, such as a recommendation, comparison, or concise technical report.
    #[serde(default)]
    pub deliverable: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeepResearchArgs {
    /// The high-level question, uncertainty, root cause, or decision the investigation must resolve.
    pub objective: String,

    /// Background state that changes the investigation or prevents the researcher from repeating known work.
    #[serde(default)]
    pub context: Option<String>,

    /// Questions that are mandatory to resolve or explicitly mark unresolved.
    #[serde(default)]
    pub must_answer: Vec<String>,

    /// Exact boundaries of the investigation. Deep research requires an explicit scope to prevent unbounded browsing.
    pub scope: String,

    /// Candidate explanations already considered. These are leads to test, never assumptions to preserve.
    #[serde(default)]
    pub hypotheses: Vec<String>,

    /// Evidence types that would materially strengthen or falsify the conclusion.
    #[serde(default)]
    pub evidence_requirements: Vec<String>,

    /// Source requirements, exclusions, or priority guidance.
    #[serde(default)]
    pub source_constraints: Option<String>,

    /// What downstream decision or action will use this research. Helps prioritize decision-relevant evidence.
    #[serde(default)]
    pub decision_context: Option<String>,

    /// Required final handoff. Be explicit about the analysis or decision support the caller needs.
    pub deliverable: String,
}
