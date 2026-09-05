use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};

use crate::{
    error::SearchError,
    research::ResearchRunner,
    tools::{DeepResearchArgs, LightSearchArgs, MediumResearchArgs},
};

#[derive(Clone)]
pub struct SearchMcp {
    runner: Arc<ResearchRunner>,
    tool_router: ToolRouter<SearchMcp>,
    native_web_search: bool,
    native_web_fetch: bool,
}

#[tool_router]
impl SearchMcp {
    pub fn new(runner: Arc<ResearchRunner>, native_web_search: bool, native_web_fetch: bool) -> Self {
        Self {
            runner,
            tool_router: Self::tool_router(),
            native_web_search,
            native_web_fetch,
        }
    }

    #[tool(description = "Resolve one narrow factual question quickly with the minimum sufficient external evidence. Use for a specific fact, current status, date, value, release, document, link, or similarly bounded lookup. Give the question and necessary context; do not invent search-engine queries. Do not use for broad comparisons, root-cause analysis, or competing explanations.")]
    async fn light_search(
        &self,
        Parameters(args): Parameters<LightSearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        let report = self.runner.light_search(args).await.map_err(to_mcp_error)?;
        Ok(text_result(report))
    }

    #[tool(description = "Investigate an objective across multiple sources and return a concise evidence-backed synthesis. This is the default research tool when a simple factual lookup is insufficient: use it for ordinary comparisons, current technical state, several related questions, or documentation plus empirical evidence. Prefer it over deep_research unless conflicting evidence, competing hypotheses, root-cause analysis, hidden subquestions, or a consequential decision genuinely require deeper investigation. The research agent owns decomposition, query generation, source selection, gap checking, and stopping.")]
    async fn medium_research(
        &self,
        Parameters(args): Parameters<MediumResearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        let report = self.runner.medium_research(args).await.map_err(to_mcp_error)?;
        Ok(text_result(report))
    }

    #[tool(description = "Perform a rigorous high-depth investigation for ambiguous, consequential, or technically difficult problems. Use only when evidence may conflict, multiple hypotheses or causal explanations must be tested, hidden subquestions may need discovery, root-cause analysis is required, or an important decision depends on the result. Do not use deep_research merely because a topic is technical or because more detail would be useful. Provide explicit scope and deliverable; hypotheses are optional leads, never assumptions.")]
    async fn deep_research(
        &self,
        Parameters(args): Parameters<DeepResearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        let report = self.runner.deep_research(args).await.map_err(to_mcp_error)?;
        Ok(text_result(report))
    }
}

#[tool_handler]
impl ServerHandler for SearchMcp {
    fn get_info(&self) -> ServerInfo {
        let mut instructions = "Three specialist research tools are available. Prefer the lowest sufficient research level: light_search for narrow factual resolution, medium_research as the default for normal multi-source synthesis, and deep_research only for rigorous evidence-driven investigation involving conflict, hypotheses, root cause, hidden subquestions, or consequential decisions. Give research briefs, not hand-written search queries.".to_string();
        if !self.native_web_search {
            instructions.push_str(" Native DSH web_search is disabled; use these Hermes tools for web discovery and research.");
        }
        if !self.native_web_fetch {
            instructions.push_str(" Native DSH web_fetch is disabled.");
        } else if !self.native_web_search {
            instructions.push_str(" Native web_fetch remains available for retrieving a known URL directly.");
        }
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_instructions(instructions)
    }
}

fn text_result(text: String) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(text)])
}

fn to_mcp_error(error: SearchError) -> McpError {
    match error {
        SearchError::InvalidInput(message) => McpError::invalid_params(message, None),
        other => McpError::internal_error(other.to_string(), None),
    }
}
