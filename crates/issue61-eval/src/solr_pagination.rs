use crate::{EngineOutcome, FrozenQuery};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

pub const AUDIT_PAGE_ROWS: &str = "5000";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedSolrRequest {
    pub q: String,
    pub fq: Vec<String>,
    pub params: BTreeMap<String, String>,
}

pub fn prepare_solr_request(query: &FrozenQuery) -> Result<PreparedSolrRequest, String> {
    let frozen = query
        .solr
        .as_ref()
        .ok_or_else(|| format!("query {} is missing its solr request block", query.query_id))?;
    require_param(&frozen.params, "sort", "id asc")?;
    require_param(&frozen.params, "fl", "id")?;
    require_param(&frozen.params, "wt", "json")?;
    if !frozen.params.contains_key("rows") {
        return Err("frozen Solr params are missing rows".to_string());
    }
    if frozen.params.contains_key("cursorMark") {
        return Err("frozen Solr params must not contain cursorMark".to_string());
    }
    let mut params = frozen.params.clone();
    params.insert("rows".to_string(), AUDIT_PAGE_ROWS.to_string());
    Ok(PreparedSolrRequest {
        q: frozen.q.clone(),
        fq: frozen.fq.clone(),
        params,
    })
}

fn require_param(
    params: &BTreeMap<String, String>,
    name: &str,
    expected: &str,
) -> Result<(), String> {
    match params.get(name) {
        Some(actual) if actual == expected => Ok(()),
        Some(actual) => Err(format!(
            "frozen Solr param {name} must be {expected:?}, found {actual:?}"
        )),
        None => Err(format!("frozen Solr params are missing {name}")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolrPage {
    pub num_found: usize,
    pub ids: Vec<String>,
    pub next_cursor_mark: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorDecision {
    Continue(String),
    Complete(Vec<String>),
}

#[derive(Debug, Default)]
pub struct CursorCollector {
    expected_count: Option<usize>,
    ids: Vec<String>,
    seen_ids: BTreeSet<String>,
    sent_cursors: BTreeSet<String>,
}

impl CursorCollector {
    pub fn accept_page(
        &mut self,
        sent_cursor: &str,
        page: SolrPage,
    ) -> Result<CursorDecision, String> {
        let page_is_empty = page.ids.is_empty();
        if !self.sent_cursors.insert(sent_cursor.to_owned()) {
            return Err(format!("cursor cycle repeated {sent_cursor:?}"));
        }
        match self.expected_count {
            Some(expected) if expected != page.num_found => {
                return Err(format!(
                    "numFound changed from {expected} to {}",
                    page.num_found
                ));
            }
            Some(_) => {}
            None => self.expected_count = Some(page.num_found),
        }
        for id in page.ids {
            if !self.seen_ids.insert(id.clone()) {
                return Err(format!("duplicate id {id:?}"));
            }
            self.ids.push(id);
        }
        if self.ids.len() > page.num_found {
            return Err(format!(
                "collected {} ids exceeds numFound {}",
                self.ids.len(),
                page.num_found
            ));
        }
        if page.next_cursor_mark == sent_cursor {
            if self.ids.len() != page.num_found {
                return Err(format!(
                    "terminal count {} differs from numFound {}",
                    self.ids.len(),
                    page.num_found
                ));
            }
            return Ok(CursorDecision::Complete(self.ids.clone()));
        }
        if page_is_empty {
            return Err("empty non-terminal cursor page".to_string());
        }
        if self.sent_cursors.contains(&page.next_cursor_mark) {
            return Err(format!(
                "non-advancing cursor cycle to {:?}",
                page.next_cursor_mark
            ));
        }
        Ok(CursorDecision::Continue(page.next_cursor_mark))
    }
}

#[derive(Deserialize)]
struct WireHeader {
    status: i64,
}

#[derive(Deserialize)]
struct WireDocument {
    id: String,
}

#[derive(Deserialize)]
struct WireBody {
    #[serde(rename = "numFound")]
    num_found: usize,
    docs: Vec<WireDocument>,
}

#[derive(Deserialize)]
struct WireResponse {
    #[serde(rename = "responseHeader")]
    header: WireHeader,
    response: Option<WireBody>,
    #[serde(rename = "nextCursorMark")]
    next_cursor_mark: Option<String>,
}

fn parse_page(body: &str) -> Result<SolrPage, EngineOutcome> {
    let parsed: WireResponse = serde_json::from_str(body)
        .map_err(|error| EngineOutcome::ParseError(format!("invalid JSON: {error}")))?;
    if parsed.header.status != 0 {
        return Err(EngineOutcome::QueryError(format!(
            "responseHeader.status={}",
            parsed.header.status
        )));
    }
    let response = parsed
        .response
        .ok_or_else(|| EngineOutcome::ParseError("missing response".to_string()))?;
    let next_cursor_mark = parsed
        .next_cursor_mark
        .ok_or_else(|| EngineOutcome::ParseError("missing nextCursorMark".to_string()))?;
    Ok(SolrPage {
        num_found: response.num_found,
        ids: response
            .docs
            .into_iter()
            .map(|document| document.id)
            .collect(),
        next_cursor_mark,
    })
}

pub fn fetch_complete_solr(
    agent: &ureq::Agent,
    core_url: &str,
    query: &FrozenQuery,
) -> EngineOutcome {
    let prepared = match prepare_solr_request(query) {
        Ok(request) => request,
        Err(reason) => return EngineOutcome::QueryError(reason),
    };
    let url = format!("{}/select", core_url.trim_end_matches('/'));
    let mut cursor = "*".to_string();
    let mut collector = CursorCollector::default();
    loop {
        let mut request = agent.get(&url).query("q", &prepared.q);
        for fq in &prepared.fq {
            request = request.query("fq", fq);
        }
        for (name, value) in &prepared.params {
            request = request.query(name, value);
        }
        request = request.query("cursorMark", &cursor);
        let response = match request.call() {
            Ok(response) => response,
            Err(ureq::Error::Status(status, _)) => {
                return EngineOutcome::HttpError(format!("status {status}"));
            }
            Err(ureq::Error::Transport(error)) => {
                return EngineOutcome::HttpError(format!("request failed: {error}"));
            }
        };
        let body = match response.into_string() {
            Ok(body) => body,
            Err(error) => {
                return EngineOutcome::HttpError(format!("read body: {error}"));
            }
        };
        let page = match parse_page(&body) {
            Ok(page) => page,
            Err(outcome) => return outcome,
        };
        match collector.accept_page(&cursor, page) {
            Ok(CursorDecision::Continue(next)) => cursor = next,
            Ok(CursorDecision::Complete(ids)) => return EngineOutcome::Ids(ids),
            Err(reason) => return EngineOutcome::ParseError(reason),
        }
    }
}
