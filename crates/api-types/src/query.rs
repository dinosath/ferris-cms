//! Strapi query-string parser (design Part V §3).
//!
//! Parses bracket-notation query strings into typed `QueryParams`:
//! `fields`, `populate` (`*`, lists, nested), `filters` (full operator set +
//! `$and`/`$or`/`$not` trees), `sort`, `pagination`, `locale`, `status`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Fully parsed query params for content endpoints.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct QueryParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub populate: Option<PopulateSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<Filter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sort: Vec<SortField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pagination: Option<PaginationParams>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<core_domain::PublicationState>,
}

impl QueryParams {
    /// Parse from a raw query string (`a=1&filters[x][$eq]=2`).
    pub fn parse(query: &str) -> Result<Self, QueryParseError> {
        let pairs = decode_pairs(query);
        Self::from_pairs(&pairs)
    }

    /// Parse from already-decoded key/value pairs.
    pub fn from_pairs(pairs: &[(String, String)]) -> Result<Self, QueryParseError> {
        let mut root = Node::default();
        for (k, v) in pairs {
            let path = split_key(k);
            if path.is_empty() {
                continue;
            }
            root.insert(&path, v.clone());
        }
        let mut out = QueryParams::default();
        for (key, node) in &root.children {
            match key.as_str() {
                "fields" => out.fields = Some(parse_string_list(node)),
                "populate" => out.populate = Some(parse_populate(node)),
                "filters" => out.filters = Some(parse_filter_node(node)?),
                "sort" => out.sort = parse_sort(node),
                "pagination" => out.pagination = Some(parse_pagination(node)),
                "locale" => {
                    out.locale = node.value.clone();
                }
                "status" => {
                    out.status = node
                        .value
                        .as_deref()
                        .and_then(core_domain::PublicationState::from_db_str);
                }
                _ => {} // ignore unknown top-level params
            }
        }
        Ok(out)
    }

    /// Effective pagination with Strapi defaults (page 1, size 25, withCount).
    pub fn effective_pagination(&self) -> EffectivePagination {
        match &self.pagination {
            Some(PaginationParams::Page {
                page,
                page_size,
                with_count,
            }) => EffectivePagination {
                limit: (*page_size).max(1),
                offset: ((*page).max(1) - 1) * (*page_size).max(1),
                page: (*page).max(1),
                page_size: (*page_size).max(1),
                with_count: with_count.unwrap_or(true),
            },
            Some(PaginationParams::Offset {
                start,
                limit,
                with_count,
            }) => {
                let limit = (*limit).max(1);
                let start = (*start).max(0);
                EffectivePagination {
                    limit,
                    offset: start,
                    page: start / limit + 1,
                    page_size: limit,
                    with_count: with_count.unwrap_or(true),
                }
            }
            None => EffectivePagination {
                limit: 25,
                offset: 0,
                page: 1,
                page_size: 25,
                with_count: true,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PopulateSpec {
    /// `populate=*`
    Star,
    /// `populate[0]=author` / `populate=author,tags`
    List(Vec<String>),
    /// `populate[author][fields][0]=name&populate[author][populate][0]=tags`
    Map(IndexMap<String, PopulateField>),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PopulateField {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub populate: Option<Box<PopulateSpec>>,
}

/// Filter tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Filter {
    And(Vec<Filter>),
    Or(Vec<Filter>),
    Not(Box<Filter>),
    Leaf {
        field: String,
        op: FilterOp,
        values: Vec<serde_json::Value>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FilterOp {
    #[serde(rename = "$eq")]
    Eq,
    #[serde(rename = "$eqi")]
    Eqi,
    #[serde(rename = "$ne")]
    Ne,
    #[serde(rename = "$lt")]
    Lt,
    #[serde(rename = "$lte")]
    Lte,
    #[serde(rename = "$gt")]
    Gt,
    #[serde(rename = "$gte")]
    Gte,
    #[serde(rename = "$in")]
    In,
    #[serde(rename = "$notIn")]
    NotIn,
    #[serde(rename = "$contains")]
    Contains,
    #[serde(rename = "$notContains")]
    NotContains,
    #[serde(rename = "$containsi")]
    ContainsI,
    #[serde(rename = "$notContainsi")]
    NotContainsI,
    #[serde(rename = "$startsWith")]
    StartsWith,
    #[serde(rename = "$startsWithi")]
    StartsWithI,
    #[serde(rename = "$endsWith")]
    EndsWith,
    #[serde(rename = "$endsWithi")]
    EndsWithI,
    #[serde(rename = "$null")]
    Null,
    #[serde(rename = "$notNull")]
    NotNull,
    #[serde(rename = "$between")]
    Between,
}

impl FilterOp {
    pub fn parse(s: &str) -> Option<Self> {
        use FilterOp::*;
        Some(match s {
            "$eq" => Eq,
            "$eqi" => Eqi,
            "$ne" => Ne,
            "$lt" => Lt,
            "$lte" => Lte,
            "$gt" => Gt,
            "$gte" => Gte,
            "$in" => In,
            "$notIn" => NotIn,
            "$contains" => Contains,
            "$notContains" => NotContains,
            "$containsi" => ContainsI,
            "$notContainsi" => NotContainsI,
            "$startsWith" => StartsWith,
            "$startsWithi" => StartsWithI,
            "$endsWith" => EndsWith,
            "$endsWithi" => EndsWithI,
            "$null" => Null,
            "$notNull" => NotNull,
            "$between" => Between,
            _ => return None,
        })
    }

    pub fn takes_list(&self) -> bool {
        matches!(self, Self::In | Self::NotIn | Self::Between)
    }

    pub fn takes_value(&self) -> bool {
        !matches!(self, Self::Null | Self::NotNull)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SortField {
    pub field: String,
    pub descending: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PaginationParams {
    Page {
        page: i64,
        #[serde(rename = "pageSize")]
        page_size: i64,
        #[serde(rename = "withCount")]
        with_count: Option<bool>,
    },
    Offset {
        start: i64,
        limit: i64,
        #[serde(rename = "withCount")]
        with_count: Option<bool>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectivePagination {
    pub limit: i64,
    pub offset: i64,
    pub page: i64,
    pub page_size: i64,
    pub with_count: bool,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq)]
pub enum QueryParseError {
    #[error("unknown filter operator `{0}`")]
    UnknownOperator(String),
    #[error("invalid filter structure at `{0}`")]
    BadFilter(String),
}

// ---------------------------------------------------------------------------
// bracket-notation tree
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct Node {
    value: Option<String>,
    children: IndexMap<String, Node>,
}

impl Node {
    fn insert(&mut self, path: &[String], value: String) {
        if path.is_empty() {
            self.value = Some(value);
            return;
        }
        self.children
            .entry(path[0].clone())
            .or_default()
            .insert(&path[1..], value);
    }

    /// Numeric-keyed children in index order (`0`, `1`, ...).
    fn indexed_children(&self) -> Vec<&Node> {
        let mut indexed: Vec<(usize, &Node)> = self
            .children
            .iter()
            .filter_map(|(k, n)| k.parse::<usize>().ok().map(|i| (i, n)))
            .collect();
        indexed.sort_by_key(|(i, _)| *i);
        indexed.into_iter().map(|(_, n)| n).collect()
    }
}

/// `filters[author][name][$eq]` -> ["filters","author","name","$eq"]
fn split_key(key: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut rest = key;
    if let Some(bracket) = rest.find('[') {
        parts.push(rest[..bracket].to_string());
        rest = &rest[bracket..];
    } else {
        parts.push(rest.to_string());
        return parts;
    }
    let mut cur = String::new();
    let mut in_bracket = false;
    for ch in rest.chars() {
        match ch {
            '[' => {
                if in_bracket && !cur.is_empty() {
                    parts.push(cur.clone());
                    cur.clear();
                }
                in_bracket = true;
            }
            ']' => {
                parts.push(cur.clone());
                cur.clear();
                in_bracket = false;
            }
            c if in_bracket => cur.push(c),
            _ => {}
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts.into_iter().filter(|p| !p.is_empty()).collect()
}

fn decode_pairs(query: &str) -> Vec<(String, String)> {
    query
        .trim_start_matches('?')
        .split('&')
        .filter(|s| !s.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = |b: u8| -> Option<u8> {
                    match b {
                        b'0'..=b'9' => Some(b - b'0'),
                        b'a'..=b'f' => Some(b - b'a' + 10),
                        b'A'..=b'F' => Some(b - b'A' + 10),
                        _ => None,
                    }
                };
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push(h * 16 + l);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------
// interpreters
// ---------------------------------------------------------------------------

fn parse_string_list(node: &Node) -> Vec<String> {
    if let Some(v) = &node.value {
        return v.split(',').map(|s| s.trim().to_string()).collect();
    }
    let indexed = node.indexed_children();
    if !indexed.is_empty() {
        return indexed
            .into_iter()
            .filter_map(|n| n.value.clone())
            .collect();
    }
    node.children.keys().cloned().collect()
}

fn parse_populate(node: &Node) -> PopulateSpec {
    if let Some(v) = &node.value {
        if v.trim() == "*" {
            return PopulateSpec::Star;
        }
        return PopulateSpec::List(v.split(',').map(|s| s.trim().to_string()).collect());
    }
    let indexed = node.indexed_children();
    if !indexed.is_empty() && indexed.len() == node.children.len() {
        return PopulateSpec::List(
            indexed
                .into_iter()
                .filter_map(|n| n.value.clone())
                .collect(),
        );
    }
    let mut map = IndexMap::new();
    for (name, child) in &node.children {
        if name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let mut field = PopulateField::default();
        for (sub_key, sub) in &child.children {
            match sub_key.as_str() {
                "fields" => field.fields = Some(parse_string_list(sub)),
                "populate" => field.populate = Some(Box::new(parse_populate(sub))),
                _ => {}
            }
        }
        map.insert(name.clone(), field);
    }
    PopulateSpec::Map(map)
}

fn parse_filter_node(node: &Node) -> Result<Filter, QueryParseError> {
    // A node is either: boolean operator(s), field(s), or a single leaf op set.
    let mut items: Vec<Filter> = Vec::new();
    for (key, child) in &node.children {
        match key.as_str() {
            "$and" => {
                let subs = child
                    .indexed_children()
                    .into_iter()
                    .map(parse_filter_node)
                    .collect::<Result<Vec<_>, _>>()?;
                items.push(Filter::And(subs));
            }
            "$or" => {
                let subs = child
                    .indexed_children()
                    .into_iter()
                    .map(parse_filter_node)
                    .collect::<Result<Vec<_>, _>>()?;
                items.push(Filter::Or(subs));
            }
            "$not" => items.push(Filter::Not(Box::new(parse_filter_node(child)?))),
            field => items.push(parse_field_filter(field, child)?),
        }
    }
    Ok(match items.len() {
        1 => items.pop().unwrap(),
        _ => Filter::And(items),
    })
}

fn parse_field_filter(field: &str, node: &Node) -> Result<Filter, QueryParseError> {
    // `filters[title]=rust` -> implicit $eq
    if let Some(v) = &node.value {
        return Ok(Filter::Leaf {
            field: field.to_string(),
            op: FilterOp::Eq,
            values: vec![coerce_scalar(v)],
        });
    }
    // nested boolean under a field: filters[title][$or][0][$contains]=a
    if node
        .children
        .keys()
        .any(|k| matches!(k.as_str(), "$and" | "$or" | "$not"))
    {
        let nested = parse_filter_node(node)?;
        return Ok(match nested {
            // keep field context by wrapping each leaf
            other => remap_field(other, field),
        });
    }
    let mut leafs = Vec::new();
    for (op_str, child) in &node.children {
        let Some(op) = FilterOp::parse(op_str) else {
            return Err(QueryParseError::UnknownOperator(op_str.clone()));
        };
        let values = parse_op_values(op, child);
        leafs.push(Filter::Leaf {
            field: field.to_string(),
            op,
            values,
        });
    }
    Ok(match leafs.len() {
        1 => leafs.pop().unwrap(),
        _ => Filter::And(leafs),
    })
}

/// Push an outer field down into nested boolean leaves:
/// `filters[x][$or][0][$eq]=a` behaves like `$or: [ {x $eq a} ]`.
fn remap_field(filter: Filter, field: &str) -> Filter {
    match filter {
        Filter::And(items) => {
            Filter::And(items.into_iter().map(|f| remap_field(f, field)).collect())
        }
        Filter::Or(items) => Filter::Or(items.into_iter().map(|f| remap_field(f, field)).collect()),
        Filter::Not(inner) => Filter::Not(Box::new(remap_field(*inner, field))),
        Filter::Leaf {
            field: _,
            op,
            values,
        } => Filter::Leaf {
            field: field.to_string(),
            op,
            values,
        },
    }
}

fn parse_op_values(op: FilterOp, node: &Node) -> Vec<serde_json::Value> {
    if let Some(v) = &node.value {
        if op.takes_list() {
            return v.split(',').map(coerce_scalar).collect();
        }
        return vec![coerce_scalar(v)];
    }
    let indexed = node.indexed_children();
    if !indexed.is_empty() {
        return indexed
            .into_iter()
            .filter_map(|n| n.value.as_deref())
            .map(coerce_scalar)
            .collect();
    }
    vec![]
}

/// Best-effort scalar coercion: ints/floats/bools become JSON primitives,
/// everything else stays a string. Column-type-aware coercion happens in
/// `dynamic-store`.
fn coerce_scalar(s: &str) -> serde_json::Value {
    let t = s.trim();
    if t == "true" {
        return serde_json::Value::Bool(true);
    }
    if t == "false" {
        return serde_json::Value::Bool(false);
    }
    if let Ok(i) = t.parse::<i64>() {
        if i.to_string() == t {
            return serde_json::Value::Number(i.into());
        }
    }
    if let Ok(f) = t.parse::<f64>() {
        if t.contains('.') || t.contains('e') || t.contains('E') {
            if let Some(n) = serde_json::Number::from_f64(f) {
                return serde_json::Value::Number(n);
            }
        }
    }
    serde_json::Value::String(s.to_string())
}

fn parse_sort(node: &Node) -> Vec<SortField> {
    let raw: Vec<String> = if let Some(v) = &node.value {
        v.split(',').map(|s| s.trim().to_string()).collect()
    } else {
        node.indexed_children()
            .into_iter()
            .filter_map(|n| n.value.clone())
            .collect()
    };
    raw.into_iter()
        .map(|item| match item.split_once(':') {
            Some((field, dir)) => SortField {
                field: field.trim().to_string(),
                descending: dir.trim().eq_ignore_ascii_case("desc"),
            },
            None => SortField {
                field: item,
                descending: false,
            },
        })
        .collect()
}

fn parse_pagination(node: &Node) -> PaginationParams {
    let get_i64 = |key: &str| -> Option<i64> {
        node.children
            .get(key)
            .and_then(|n| n.value.as_deref())
            .and_then(|v| v.parse().ok())
    };
    let get_bool = |key: &str| -> Option<bool> {
        node.children
            .get(key)
            .and_then(|n| n.value.as_deref())
            .map(|v| v == "true" || v == "1")
    };
    if node.children.contains_key("start") || node.children.contains_key("limit") {
        PaginationParams::Offset {
            start: get_i64("start").unwrap_or(0),
            limit: get_i64("limit").unwrap_or(25),
            with_count: get_bool("withCount"),
        }
    } else {
        PaginationParams::Page {
            page: get_i64("page").unwrap_or(1),
            page_size: get_i64("pageSize").unwrap_or(25),
            with_count: get_bool("withCount"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_splitting() {
        assert_eq!(
            split_key("filters[author][name][$eq]"),
            vec!["filters", "author", "name", "$eq"]
        );
        assert_eq!(split_key("populate"), vec!["populate"]);
        assert_eq!(split_key("sort[0]"), vec!["sort", "0"]);
    }

    #[test]
    fn decodes_pairs() {
        let pairs = decode_pairs("a=hello%20world&b=x+y");
        assert_eq!(pairs[0], ("a".into(), "hello world".into()));
        assert_eq!(pairs[1], ("b".into(), "x y".into()));
    }

    #[test]
    fn full_fixture() {
        let q = QueryParams::parse(
            "fields[0]=title&fields[1]=slug\
             &populate=author\
             &filters[title][$contains]=rust\
             &filters[views][$gte]=10\
             &sort[0]=title:desc\
             &pagination[page]=2&pagination[pageSize]=5\
             &locale=fr&status=published",
        )
        .unwrap();
        assert_eq!(q.fields.as_ref().unwrap(), &vec!["title", "slug"]);
        assert_eq!(q.populate, Some(PopulateSpec::List(vec!["author".into()])));
        assert_eq!(q.sort[0].field, "title");
        assert!(q.sort[0].descending);
        assert_eq!(q.locale.as_deref(), Some("fr"));
        assert_eq!(q.status, Some(core_domain::PublicationState::Published));
        let eff = q.effective_pagination();
        assert_eq!((eff.limit, eff.offset), (5, 5));

        let f = q.filters.clone().unwrap();
        match f {
            Filter::And(items) => {
                assert_eq!(items.len(), 2);
                assert!(matches!(
                    &items[0],
                    Filter::Leaf { field, op: FilterOp::Contains, values }
                        if field == "title" && values == &vec![json!("rust")]
                ));
                assert!(matches!(
                    &items[1],
                    Filter::Leaf { field, op: FilterOp::Gte, values }
                        if field == "views" && values == &vec![json!(10)]
                ));
            }
            other => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn populate_star_and_nested() {
        let q = QueryParams::parse("populate=*").unwrap();
        assert_eq!(q.populate, Some(PopulateSpec::Star));

        let q = QueryParams::parse(
            "populate[author][fields][0]=name&populate[author][populate][0]=avatar&populate[tags]=x",
        )
        .unwrap();
        match q.populate.unwrap() {
            PopulateSpec::Map(map) => {
                let author = &map["author"];
                assert_eq!(author.fields.as_ref().unwrap(), &vec!["name".to_string()]);
                assert_eq!(
                    author.populate.as_deref(),
                    Some(&PopulateSpec::List(vec!["avatar".into()]))
                );
                assert!(map.contains_key("tags"));
            }
            other => panic!("expected Map, got {other:?}"),
        }
    }

    #[test]
    fn boolean_filter_trees() {
        let q = QueryParams::parse(
            "filters[$or][0][title][$containsi]=rust&filters[$or][1][views][$gt]=5&filters[$not][archived][$eq]=true",
        )
        .unwrap();
        match q.filters.unwrap() {
            Filter::And(items) => {
                assert_eq!(items.len(), 2);
                assert!(matches!(&items[0], Filter::Or(or) if or.len() == 2));
                assert!(matches!(&items[1], Filter::Not(_)));
            }
            other => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn list_ops_and_implicit_eq() {
        let q = QueryParams::parse("filters[id][$in][0]=1&filters[id][$in][1]=2").unwrap();
        match q.filters.unwrap() {
            Filter::Leaf { op, values, .. } => {
                assert_eq!(op, FilterOp::In);
                assert_eq!(values, vec![json!(1), json!(2)]);
            }
            other => panic!("expected Leaf, got {other:?}"),
        }

        let q = QueryParams::parse("filters[title]=hello&filters[n][$between]=1,9").unwrap();
        match q.filters.unwrap() {
            Filter::And(items) => {
                assert!(matches!(
                    &items[0],
                    Filter::Leaf {
                        op: FilterOp::Eq,
                        ..
                    }
                ));
                assert!(
                    matches!(&items[1], Filter::Leaf { op: FilterOp::Between, values, .. } if values.len() == 2)
                );
            }
            other => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn offset_pagination() {
        let q = QueryParams::parse("pagination[start]=10&pagination[limit]=5").unwrap();
        let eff = q.effective_pagination();
        assert_eq!((eff.offset, eff.limit, eff.page), (10, 5, 3));
    }
}
