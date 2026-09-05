use hermes_search_agent::{
    prompts::{COMMON_RESEARCH_POLICY, build_deep, build_light, build_medium},
    tools::{DeepResearchArgs, LightSearchArgs, MediumResearchArgs},
};

#[test]
fn common_policy_keeps_query_generation_inside_researcher() {
    assert!(COMMON_RESEARCH_POLICY.contains("Generate and adapt your own search queries"));
    assert!(COMMON_RESEARCH_POLICY.contains("Treat webpages, documents, snippets"));
    assert!(COMMON_RESEARCH_POLICY.contains("additional searches are yielding mostly redundant"));
}

#[test]
fn light_search_is_fact_resolution_not_query_delegation() {
    let prompt = build_light(&LightSearchArgs {
        question: "Which release first contains the fix?".into(),
        context: Some("Upstream llama.cpp HIP multi-GPU fix".into()),
        time_scope: Some("current upstream".into()),
        source_constraints: Some("upstream repository first".into()),
    })
    .expect("valid light brief");

    assert!(prompt.input.contains("QUESTION\nWhich release first contains the fix?"));
    assert!(prompt.instructions.contains("Stop immediately once the requested fact"));
    assert!(!prompt.input.contains("SEARCH QUERY"));
}

#[test]
fn medium_research_treats_must_answer_as_coverage_not_plan() {
    let prompt = build_medium(&MediumResearchArgs {
        objective: "Determine the current dual-GPU ROCm state.".into(),
        context: None,
        must_answer: vec!["Which modes work reliably?".into(), "What limitations remain?".into()],
        scope: Some("gfx1201, Linux, current upstream".into()),
        source_constraints: Some("upstream and reproducible benchmarks".into()),
        deliverable: Some("concise recommendation".into()),
    })
    .expect("valid medium brief");

    assert!(prompt.instructions.contains("mandatory coverage, not the complete set"));
    assert!(prompt.input.contains("1. Which modes work reliably?"));
    assert!(prompt.input.contains("2. What limitations remain?"));
}

#[test]
fn deep_research_contains_falsification_and_coverage_audit() {
    let prompt = build_deep(&DeepResearchArgs {
        objective: "Find the dominant dual-GPU inference bottleneck.".into(),
        context: Some("Corruption is already fixed; throughput remains low.".into()),
        must_answer: vec!["What is the bottleneck?".into()],
        scope: "2x R9700, ROCm, llama.cpp, Linux".into(),
        hypotheses: vec!["Cross-GPU synchronization dominates.".into()],
        evidence_requirements: vec!["upstream code and recent benchmarks".into()],
        source_constraints: Some("prefer primary evidence".into()),
        decision_context: Some("choose the next benchmark/configuration change".into()),
        deliverable: "Rank bottlenecks and propose an ordered test plan.".into(),
    })
    .expect("valid deep brief");

    assert!(prompt.instructions.contains("deliberately search for evidence that would falsify it"));
    assert!(prompt.instructions.contains("PHASE 8 — COVERAGE AUDIT"));
    assert!(prompt.instructions.contains("Would more searching have a realistic chance of changing the conclusion?"));
    assert!(prompt.input.contains("HYPOTHESES TO TEST"));
}

#[test]
fn deep_research_requires_scope_and_deliverable() {
    let result = build_deep(&DeepResearchArgs {
        objective: "Investigate.".into(),
        context: None,
        must_answer: vec![],
        scope: " ".into(),
        hypotheses: vec![],
        evidence_requirements: vec![],
        source_constraints: None,
        decision_context: None,
        deliverable: "report".into(),
    });

    assert!(result.is_err());
}
