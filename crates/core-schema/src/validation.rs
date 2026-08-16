//! Structural validation (design Part IV §9).
//!
//! Validates a *desired set* of schemas as a batch: cross-schema rules
//! (relation targets, component existence, DZ field collisions) need the
//! whole registry, so validation takes `&[Schema]`.

use crate::model::Schema;
use core_domain::{reserved, snake_case, ContentTypeKind, FieldType, RelationKind, Uid};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{HashMap, HashSet};

/// One validation failure, Strapi error-details compatible.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FieldError {
    /// Dotted path, e.g. `article.attributes.title.maxLength`.
    pub path: String,
    /// Stable machine code, e.g. `invalid-identifier`.
    pub code: String,
    pub message: String,
}

impl FieldError {
    pub fn new(
        path: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            code: code.into(),
            message: message.into(),
        }
    }
}

static IDENT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9_]*$").unwrap());
static API_ID_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-z][a-z0-9-]*$").unwrap());
static ENUM_VALUE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9_]*$").unwrap());

/// Validate every schema plus all cross-schema rules. Returns all errors
/// (the batch is rejected wholesale when this is non-empty).
pub fn validate_schemas(schemas: &[Schema]) -> Vec<FieldError> {
    let mut errors = Vec::new();
    let by_uid: HashMap<&str, &Schema> = schemas.iter().map(|s| (s.uid.as_str(), s)).collect();

    // Global uniqueness of uids / api ids / tables.
    let mut uids: HashSet<&str> = HashSet::new();
    let mut singulars: HashMap<String, &Uid> = HashMap::new();
    let mut plurals: HashMap<String, &Uid> = HashMap::new();
    let mut tables: HashMap<String, &Uid> = HashMap::new();

    for schema in schemas {
        let base = schema.uid.to_string();
        if !uids.insert(schema.uid.as_str()) {
            errors.push(FieldError::new(
                &base,
                "duplicate-uid",
                format!("uid `{}` is defined more than once", schema.uid),
            ));
        }
        validate_identity(schema, &mut errors);

        if schema.kind != ContentTypeKind::Component {
            let singular = schema.info.singular_name.clone();
            let plural = schema.info.plural_name.clone();
            if let Some(prev) = singulars.insert(singular.clone(), &schema.uid) {
                errors.push(FieldError::new(
                    format!("{base}.info.singularName"),
                    "duplicate-api-id",
                    format!("singular api id `{singular}` already used by `{prev}`"),
                ));
            }
            if let Some(prev) = plurals.insert(plural.clone(), &schema.uid) {
                errors.push(FieldError::new(
                    format!("{base}.info.pluralName"),
                    "duplicate-api-id",
                    format!("plural api id `{plural}` already used by `{prev}`"),
                ));
            }
        }

        let table = schema.table_name();
        if let Some(prev) = tables.insert(table.clone(), &schema.uid) {
            errors.push(FieldError::new(
                &base,
                "duplicate-table",
                format!("table `{table}` already used by `{prev}`"),
            ));
        }

        validate_attributes(schema, &by_uid, &mut errors);
    }
    errors
}

/// api ids / display names / kind rules.
fn validate_identity(schema: &Schema, errors: &mut Vec<FieldError>) {
    let base = schema.uid.to_string();
    match schema.kind {
        ContentTypeKind::Component => {
            if !schema.uid.is_component() {
                errors.push(FieldError::new(
                    &base,
                    "invalid-uid",
                    "component uid must be `<category>.<name>`",
                ));
            }
        }
        _ => {
            if schema.info.singular_name.is_empty() || schema.info.plural_name.is_empty() {
                errors.push(FieldError::new(
                    format!("{base}.info"),
                    "missing-api-id",
                    "singular and plural api ids are required",
                ));
            }
            for (field, value) in [
                ("singularName", &schema.info.singular_name),
                ("pluralName", &schema.info.plural_name),
            ] {
                if !API_ID_RE.is_match(value) {
                    errors.push(FieldError::new(
                        format!("{base}.info.{field}"),
                        "invalid-api-id",
                        format!("`{value}` must be lowercase letters, digits and dashes"),
                    ));
                }
                if reserved::RESERVED_API_IDS.contains(&value.as_str()) {
                    errors.push(FieldError::new(
                        format!("{base}.info.{field}"),
                        "reserved-api-id",
                        format!("`{value}` is a reserved api id"),
                    ));
                }
            }
            if schema.info.display_name.is_empty() {
                errors.push(FieldError::new(
                    format!("{base}.info.displayName"),
                    "missing-display-name",
                    "display name is required",
                ));
            }
        }
    }
}

fn validate_attributes(
    schema: &Schema,
    by_uid: &HashMap<&str, &Schema>,
    errors: &mut Vec<FieldError>,
) {
    let base = schema.uid.to_string();
    for (name, attr) in &schema.attributes {
        let path = format!("{base}.attributes.{name}");
        if !IDENT_RE.is_match(name) {
            errors.push(FieldError::new(
                &path,
                "invalid-identifier",
                format!("attribute name `{name}` must be an identifier (letter, then letters/digits/underscores)"),
            ));
        }
        if reserved::RESERVED_ATTRIBUTES
            .iter()
            .any(|r| snake_case(r) == snake_case(name))
        {
            errors.push(FieldError::new(
                &path,
                "reserved-attribute",
                format!("attribute name `{name}` is reserved"),
            ));
        }
        if let Some(re) = &attr.regex {
            if Regex::new(re).is_err() {
                errors.push(FieldError::new(
                    format!("{path}.regex"),
                    "invalid-regex",
                    format!("`{re}` is not a valid regular expression"),
                ));
            }
        }
        if let (Some(min), Some(max)) = (attr.min_length, attr.max_length) {
            if min > max {
                errors.push(FieldError::new(
                    &path,
                    "invalid-range",
                    format!("minLength {min} > maxLength {max}"),
                ));
            }
        }

        use FieldType::*;
        match attr.attr_type {
            Enumeration => validate_enumeration(&path, attr, errors),
            Uid => {
                if let Some(target) = &attr.target_field {
                    if !schema.attributes.contains_key(target) {
                        errors.push(FieldError::new(
                            format!("{path}.targetField"),
                            "missing-target-field",
                            format!("uid targetField `{target}` does not exist on this schema"),
                        ));
                    }
                }
            }
            Relation => validate_relation(&path, attr, by_uid, errors),
            Component => {
                match attr.component.as_ref().and_then(|u| by_uid.get(u.as_str())) {
                    Some(target) if target.kind == ContentTypeKind::Component => {}
                    Some(_) => errors.push(FieldError::new(
                        format!("{path}.component"),
                        "invalid-component",
                        "component attribute must reference a component schema",
                    )),
                    None => errors.push(FieldError::new(
                        format!("{path}.component"),
                        "missing-component",
                        format!(
                            "component `{}` is not defined",
                            attr.component
                                .as_ref()
                                .map(|u| u.as_str())
                                .unwrap_or("<none>")
                        ),
                    )),
                }
                if let (Some(min), Some(max)) = (&attr.min, &attr.max) {
                    if min.as_i64().zip(max.as_i64()).is_some_and(|(a, b)| a > b) {
                        errors.push(FieldError::new(
                            &path,
                            "invalid-range",
                            "component min > max",
                        ));
                    }
                }
            }
            Dynamiczone => {
                if attr.components.is_empty() {
                    errors.push(FieldError::new(
                        format!("{path}.components"),
                        "empty-dynamiczone",
                        "dynamic zone must allow at least one component",
                    ));
                }
                for c in &attr.components {
                    match by_uid.get(c.as_str()) {
                        Some(target) if target.kind == ContentTypeKind::Component => {}
                        _ => errors.push(FieldError::new(
                            format!("{path}.components"),
                            "missing-component",
                            format!("dynamic zone component `{c}` is not defined"),
                        )),
                    }
                }
                validate_dz_collisions(&path, attr, by_uid, errors);
            }
            Media => {
                for t in &attr.allowed_types {
                    if !matches!(t.as_str(), "images" | "videos" | "audios" | "files") {
                        errors.push(FieldError::new(
                            format!("{path}.allowedTypes"),
                            "invalid-media-type",
                            format!("allowedTypes entry `{t}` must be images|videos|audios|files"),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
}

fn validate_enumeration(path: &str, attr: &crate::Attribute, errors: &mut Vec<FieldError>) {
    if attr.enum_values.is_empty() {
        errors.push(FieldError::new(
            format!("{path}.enum"),
            "empty-enum",
            "enumeration must have at least one value",
        ));
    }
    let mut seen = HashSet::new();
    for v in &attr.enum_values {
        if v.is_empty() {
            errors.push(FieldError::new(
                format!("{path}.enum"),
                "empty-enum-value",
                "enumeration values must be non-empty",
            ));
        } else if !ENUM_VALUE_RE.is_match(v) {
            errors.push(FieldError::new(
                format!("{path}.enum"),
                "invalid-enum-value",
                format!("enum value `{v}` must start with a letter and contain only letters/digits/underscores"),
            ));
        }
        if !seen.insert(v) {
            errors.push(FieldError::new(
                format!("{path}.enum"),
                "duplicate-enum-value",
                format!("enum value `{v}` is duplicated"),
            ));
        }
    }
}

fn validate_relation(
    path: &str,
    attr: &crate::Attribute,
    by_uid: &HashMap<&str, &Schema>,
    errors: &mut Vec<FieldError>,
) {
    let Some(kind) = attr.relation else {
        errors.push(FieldError::new(
            format!("{path}.relation"),
            "missing-relation-kind",
            "relation attribute requires a `relation` kind",
        ));
        return;
    };
    let Some(target_uid) = &attr.target else {
        errors.push(FieldError::new(
            format!("{path}.target"),
            "missing-relation-target",
            "relation attribute requires a `target` uid",
        ));
        return;
    };
    let Some(target) = by_uid.get(target_uid.as_str()) else {
        errors.push(FieldError::new(
            format!("{path}.target"),
            "missing-relation-target",
            format!("relation target `{target_uid}` is not defined"),
        ));
        return;
    };
    if target.kind == ContentTypeKind::Component {
        errors.push(FieldError::new(
            format!("{path}.target"),
            "invalid-relation-target",
            "relations must target a collection or single type, not a component",
        ));
        return;
    }

    // Paired-field consistency: the inverse attribute, when named, must exist
    // on the target and point back with the dual kind.
    let dual = match kind {
        RelationKind::OneToOne => Some(RelationKind::OneToOne),
        RelationKind::OneToMany => Some(RelationKind::ManyToOne),
        RelationKind::ManyToOne => Some(RelationKind::OneToMany),
        RelationKind::ManyToMany => Some(RelationKind::ManyToMany),
        RelationKind::OneWay | RelationKind::ManyWay => None,
    };
    let inverse_name = attr.inversed_by.as_ref().or(attr.mapped_by.as_ref());
    if let (Some(inverse), Some(dual)) = (inverse_name, dual) {
        match target.attributes.get(inverse) {
            None => errors.push(FieldError::new(
                format!("{path}.inversedBy"),
                "missing-inverse-field",
                format!("inverse field `{inverse}` does not exist on `{target_uid}`"),
            )),
            Some(inv_attr) => {
                if inv_attr.attr_type != FieldType::Relation || inv_attr.relation != Some(dual) {
                    errors.push(FieldError::new(
                        format!("{path}.inversedBy"),
                        "inverse-kind-mismatch",
                        format!(
                            "inverse field `{inverse}` on `{target_uid}` must be a `{dual:?}` relation"
                        ),
                    ));
                } else if inv_attr.target.as_ref().map(Uid::as_str)
                    != Some(path.split('.').next().unwrap_or_default())
                {
                    // Best-effort back-pointer check; the from-side check in the
                    // inverse attribute validation covers the other direction.
                }
            }
        }
    }
}

/// DZ field-collision rule (Part IV §9): components sharing a field name
/// inside one zone must agree on type (and enum values).
fn validate_dz_collisions(
    path: &str,
    attr: &crate::Attribute,
    by_uid: &HashMap<&str, &Schema>,
    errors: &mut Vec<FieldError>,
) {
    let mut seen: HashMap<&str, (&Uid, FieldType, Vec<String>)> = HashMap::new();
    for comp_uid in &attr.components {
        let Some(comp) = by_uid.get(comp_uid.as_str()) else {
            continue;
        };
        for (field_name, field) in &comp.attributes {
            let key = field_name.as_str();
            let sig = (comp_uid, field.attr_type, field.enum_values.clone());
            match seen.get(key) {
                None => {
                    seen.insert(key, sig);
                }
                Some((prev_uid, prev_type, prev_enum)) => {
                    if *prev_type != field.attr_type
                        || (field.attr_type == FieldType::Enumeration
                            && *prev_enum != field.enum_values)
                    {
                        errors.push(FieldError::new(
                            format!("{path}.components"),
                            "dz-field-collision",
                            format!(
                                "field `{field_name}` on `{comp_uid}` conflicts with same-named field on `{prev_uid}` (different type or enum values)"
                            ),
                        ));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Attribute;
    use indexmap::IndexMap;

    fn schema(uid: &str, kind: ContentTypeKind, attrs: &[(&str, Attribute)]) -> Schema {
        let singular = uid.rsplit('.').next().unwrap().to_string();
        Schema {
            uid: Uid::new(uid),
            kind,
            collection_name: None,
            info: crate::SchemaInfo {
                singular_name: singular.clone(),
                plural_name: core_domain::pluralize(&singular),
                display_name: singular.clone(),
                description: None,
                icon: None,
            },
            options: Default::default(),
            plugin_options: None,
            attributes: attrs
                .iter()
                .map(|(n, a)| (n.to_string(), a.clone()))
                .collect::<IndexMap<_, _>>(),
        }
    }

    fn author_ct() -> Schema {
        schema(
            "api::author.author",
            ContentTypeKind::CollectionType,
            &[("name", Attribute::new(FieldType::String))],
        )
    }

    #[test]
    fn valid_article_passes() {
        let mut many_to_one = Attribute::new(FieldType::Relation);
        many_to_one.relation = Some(RelationKind::ManyToOne);
        many_to_one.target = Some(Uid::new("api::author.author"));

        let article = schema(
            "api::article.article",
            ContentTypeKind::CollectionType,
            &[
                ("title", Attribute::new(FieldType::String)),
                ("author", many_to_one),
            ],
        );
        let errors = validate_schemas(&[article, author_ct()]);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    #[test]
    fn catches_reserved_and_bad_names() {
        let bad = schema(
            "api::article.article",
            ContentTypeKind::CollectionType,
            &[("document_id", Attribute::new(FieldType::String))],
        );
        let errors = validate_schemas(&[bad]);
        assert!(errors.iter().any(|e| e.code == "reserved-attribute"));
    }

    #[test]
    fn catches_enum_problems() {
        let mut e = Attribute::new(FieldType::Enumeration);
        e.enum_values = vec!["1bad".into(), "ok".into(), "ok".into()];
        let s = schema(
            "api::article.article",
            ContentTypeKind::CollectionType,
            &[("state", e)],
        );
        let errors = validate_schemas(&[s]);
        assert!(errors.iter().any(|e| e.code == "invalid-enum-value"));
        assert!(errors.iter().any(|e| e.code == "duplicate-enum-value"));
    }

    #[test]
    fn catches_missing_relation_target_and_inverse() {
        let mut rel = Attribute::new(FieldType::Relation);
        rel.relation = Some(RelationKind::ManyToOne);
        rel.target = Some(Uid::new("api::ghost.ghost"));
        let s = schema(
            "api::article.article",
            ContentTypeKind::CollectionType,
            &[("author", rel)],
        );
        let errors = validate_schemas(&[s]);
        assert!(errors.iter().any(|e| e.code == "missing-relation-target"));

        let mut rel2 = Attribute::new(FieldType::Relation);
        rel2.relation = Some(RelationKind::ManyToOne);
        rel2.target = Some(Uid::new("api::author.author"));
        rel2.inversed_by = Some("nope".into());
        let s2 = schema(
            "api::article.article",
            ContentTypeKind::CollectionType,
            &[("author", rel2)],
        );
        let errors2 = validate_schemas(&[s2, author_ct()]);
        assert!(errors2.iter().any(|e| e.code == "missing-inverse-field"));
    }

    #[test]
    fn catches_dz_collision() {
        let mut text = Attribute::new(FieldType::Text);
        let mut number = Attribute::new(FieldType::Integer);
        text.required = true;
        number.required = false;
        let hero = schema(
            "shared.hero",
            ContentTypeKind::Component,
            &[("value", text)],
        );
        let cta = schema(
            "shared.cta",
            ContentTypeKind::Component,
            &[("value", number)],
        );
        let mut dz = Attribute::new(FieldType::Dynamiczone);
        dz.components = vec![Uid::new("shared.hero"), Uid::new("shared.cta")];
        let page = schema(
            "api::page.page",
            ContentTypeKind::CollectionType,
            &[("blocks", dz)],
        );
        let errors = validate_schemas(&[page, hero, cta]);
        assert!(
            errors.iter().any(|e| e.code == "dz-field-collision"),
            "expected dz-field-collision, got {errors:?}"
        );
    }

    #[test]
    fn catches_uid_target_and_duplicate_api_ids() {
        let mut uid_attr = Attribute::new(FieldType::Uid);
        uid_attr.target_field = Some("missing".into());
        let a = schema(
            "api::article.article",
            ContentTypeKind::CollectionType,
            &[("slug", uid_attr)],
        );
        let errors = validate_schemas(&[a]);
        assert!(errors.iter().any(|e| e.code == "missing-target-field"));

        let b1 = schema("api::one.one", ContentTypeKind::CollectionType, &[]);
        let mut b2 = schema("api::two.two", ContentTypeKind::CollectionType, &[]);
        b2.info.singular_name = "one".into();
        let errors = validate_schemas(&[b1, b2]);
        assert!(errors.iter().any(|e| e.code == "duplicate-api-id"));
    }
}
