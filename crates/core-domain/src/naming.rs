//! Deterministic physical naming (design Part IV §5).

/// Convert a name to `snake_case`. Handles camelCase, PascalCase, kebab-case
/// and already-snake input. Non-ASCII-alphanumerics collapse to `_`.
pub fn snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut prev_lower_or_digit = false;
    let mut prev_underscore = true; // avoid leading underscore
    for ch in s.chars() {
        if ch.is_ascii_uppercase() {
            if prev_lower_or_digit {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            prev_lower_or_digit = false;
            prev_underscore = false;
        } else if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
            prev_lower_or_digit = true;
            prev_underscore = false;
        } else {
            if !prev_underscore && !out.is_empty() {
                out.push('_');
            }
            prev_underscore = true;
            prev_lower_or_digit = false;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

/// `snake_case` -> `PascalCase` ("blog_post" -> "BlogPost").
pub fn pascal_case(s: &str) -> String {
    let snake = snake_case(s);
    let mut out = String::with_capacity(snake.len());
    for part in snake.split('_') {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
        }
    }
    out
}

/// Very small English pluralizer, deterministic and dependency-free.
/// Good enough for `article -> articles`, `category -> categories`,
/// `box -> boxes`, `child -> children`-style CMS names.
pub fn pluralize(s: &str) -> String {
    if s.is_empty() {
        return s.to_string();
    }
    let lower = s.to_ascii_lowercase();
    let irregular: Option<&str> = match lower.as_str() {
        "child" => Some("children"),
        "person" => Some("people"),
        "man" => Some("men"),
        "woman" => Some("women"),
        "mouse" => Some("mice"),
        "foot" => Some("feet"),
        "tooth" => Some("teeth"),
        _ => None,
    };
    if let Some(rep) = irregular {
        return rep.to_string();
    }
    if lower.ends_with("is") && lower.len() > 2 {
        return format!("{}es", &s[..s.len() - 2]);
    }
    if lower.ends_with("s")
        || lower.ends_with("x")
        || lower.ends_with("z")
        || lower.ends_with("ch")
        || lower.ends_with("sh")
    {
        return format!("{s}es");
    }
    if lower.ends_with('y') && lower.len() > 1 {
        let prev = lower.as_bytes()[lower.len() - 2] as char;
        if !"aeiou".contains(prev) {
            return format!("{}ies", &s[..s.len() - 1]);
        }
    }
    if lower.ends_with("fe") {
        return format!("{}ves", &s[..s.len() - 2]);
    }
    if lower.ends_with('f')
        && !lower.ends_with("ff")
        && !lower.ends_with("roof")
        && !lower.ends_with("chief")
    {
        return format!("{}ves", &s[..s.len() - 1]);
    }
    if lower.ends_with("is") && lower.len() > 2 {
        return format!("{}es", &s[..s.len() - 2]);
    }
    format!("{s}s")
}

/// Naive singularizer (inverse of [`pluralize`] for the same rules).
pub fn singularize(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let irregular: Option<&str> = match lower.as_str() {
        "children" => Some("child"),
        "people" => Some("person"),
        "men" => Some("man"),
        "women" => Some("woman"),
        "mice" => Some("mouse"),
        "feet" => Some("foot"),
        "teeth" => Some("tooth"),
        _ => None,
    };
    if let Some(rep) = irregular {
        return rep.to_string();
    }
    if lower.ends_with("ies") && s.len() > 3 {
        return format!("{}y", &s[..s.len() - 3]);
    }
    if lower.ends_with("ves") && s.len() > 3 {
        return format!("{}f", &s[..s.len() - 3]);
    }
    if lower.ends_with("ches") || lower.ends_with("shes") {
        return s[..s.len() - 2].to_string();
    }
    if (lower.ends_with("ses") || lower.ends_with("xes") || lower.ends_with("zes"))
        && s.len() > 3
    {
        return s[..s.len() - 2].to_string();
    }
    if lower.ends_with('s') && !lower.ends_with("ss") && s.len() > 1 {
        return s[..s.len() - 1].to_string();
    }
    s.to_string()
}

/// SQL column for an attribute name; reserved words get a trailing `_`.
pub fn column_name(attr: &str) -> String {
    let snake = snake_case(attr);
    if is_reserved_sql(&snake) {
        format!("{snake}_")
    } else {
        snake
    }
}

fn is_reserved_sql(word: &str) -> bool {
    matches!(
        word,
        "order" | "group" | "table" | "select" | "where" | "index" | "default" | "primary"
            | "references" | "unique" | "check" | "constraint" | "column" | "key" | "values"
            | "user" | "type" | "when" | "case" | "limit" | "offset"
    )
}

/// Collection table: `ct_<snake plural>`.
pub fn collection_table(plural: &str) -> String {
    format!("ct_{}", snake_case(plural))
}

/// Single type table: `ct_<snake singular>`.
pub fn single_table(singular: &str) -> String {
    format!("ct_{}", snake_case(singular))
}

/// Component table: `cmp_<snake category>_<snake name>`.
pub fn component_table(category: &str, name: &str) -> String {
    format!("cmp_{}_{}", snake_case(category), snake_case(name))
}

/// FK column for a relation field: `<snake attr>_id`.
pub fn fk_column(attr: &str) -> String {
    format!("{}_id", column_name(attr))
}

/// m2m / many-way join table: `ct_<a_plural>_<attr>_links`.
pub fn relation_join_table(owner_table: &str, attr: &str) -> String {
    format!("{}_{}_links", owner_table, column_name(attr))
}

/// Media link table: `<host>_<attr>_files_links`.
pub fn media_link_table(host_table: &str, attr: &str) -> String {
    format!("{}_{}_files_links", host_table, column_name(attr))
}

/// Component / dynamic-zone link table for a host: `<host>_components`.
pub fn component_link_table(host_table: &str) -> String {
    format!("{host_table}_components")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_cases() {
        assert_eq!(snake_case("blogPost"), "blog_post");
        assert_eq!(snake_case("BlogPost"), "blog_post");
        assert_eq!(snake_case("blog-post"), "blog_post");
        assert_eq!(snake_case("blog_post"), "blog_post");
        assert_eq!(snake_case("SEO"), "seo");
        assert_eq!(snake_case("myURL2Value"), "my_url2_value");
        assert_eq!(snake_case("  weird  name "), "weird_name");
    }

    #[test]
    fn pascal_cases() {
        assert_eq!(pascal_case("blog_post"), "BlogPost");
        assert_eq!(pascal_case("article"), "Article");
    }

    #[test]
    fn plurals() {
        assert_eq!(pluralize("article"), "articles");
        assert_eq!(pluralize("category"), "categories");
        assert_eq!(pluralize("box"), "boxes");
        assert_eq!(pluralize("church"), "churches");
        assert_eq!(pluralize("child"), "children");
        assert_eq!(pluralize("analysis"), "analyses");
        assert_eq!(pluralize("knife"), "knives");
        assert_eq!(pluralize("day"), "days");
        assert_eq!(singularize("articles"), "article");
        assert_eq!(singularize("categories"), "category");
        assert_eq!(singularize("boxes"), "box");
    }

    #[test]
    fn physical_names() {
        assert_eq!(collection_table("articles"), "ct_articles");
        assert_eq!(single_table("homepage"), "ct_homepage");
        assert_eq!(component_table("shared", "seo"), "cmp_shared_seo");
        assert_eq!(fk_column("author"), "author_id");
        assert_eq!(column_name("order"), "order_");
        assert_eq!(
            relation_join_table("ct_articles", "tags"),
            "ct_articles_tags_links"
        );
        assert_eq!(
            media_link_table("ct_articles", "cover"),
            "ct_articles_cover_files_links"
        );
        assert_eq!(component_link_table("ct_articles"), "ct_articles_components");
    }
}
