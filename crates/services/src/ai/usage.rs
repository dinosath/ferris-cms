//! AI usage + audit accounting.

use ai::AiUsage;
use db::entities::ai_usage;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set};

use crate::{AppContext, ServiceError};

/// Record one AI operation's token usage + audit line.
#[allow(clippy::too_many_arguments)]
pub async fn log_usage(
    ctx: &AppContext,
    user_id: i64,
    provider_id: Option<i64>,
    model: Option<&str>,
    feature: Option<&str>,
    usage: AiUsage,
    status: Option<&str>,
) -> Result<(), ServiceError> {
    let now = chrono::Utc::now();
    let row = ai_usage::ActiveModel {
        user_id: Set(user_id),
        provider_id: Set(provider_id),
        model: Set(model.map(|s| s.to_string())),
        feature: Set(feature.map(|s| s.to_string())),
        input_tokens: Set(usage.input_tokens as i64),
        output_tokens: Set(usage.output_tokens as i64),
        total_tokens: Set(usage.total_tokens as i64),
        status: Set(status.map(|s| s.to_string())),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    let _ = row;
    Ok(())
}

/// List usage records, newest first. `user_id` filters to one user when given.
pub async fn list_usage(
    ctx: &AppContext,
    user_id: Option<i64>,
    limit: u64,
) -> Result<Vec<serde_json::Value>, ServiceError> {
    let mut q = ai_usage::Entity::find().order_by_desc(ai_usage::Column::Id).limit(limit);
    if let Some(uid) = user_id {
        q = q.filter(ai_usage::Column::UserId.eq(uid));
    }
    let rows = q.all(&ctx.db).await?;
    Ok(rows.into_iter().map(usage_dto).collect())
}

/// Aggregate usage totals (tokens + request count), optionally per user.
pub async fn usage_summary(
    ctx: &AppContext,
    user_id: Option<i64>,
) -> Result<serde_json::Value, ServiceError> {
    let rows = list_usage(ctx, user_id, 1_000_000).await?;
    let requests = rows.len();
    let mut input = 0i64;
    let mut output = 0i64;
    for r in &rows {
        input += r["inputTokens"].as_i64().unwrap_or(0);
        output += r["outputTokens"].as_i64().unwrap_or(0);
    }
    Ok(serde_json::json!({
        "requests": requests,
        "inputTokens": input,
        "outputTokens": output,
        "totalTokens": input + output,
    }))
}

fn usage_dto(row: ai_usage::Model) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "userId": row.user_id,
        "providerId": row.provider_id,
        "model": row.model,
        "feature": row.feature,
        "inputTokens": row.input_tokens,
        "outputTokens": row.output_tokens,
        "totalTokens": row.total_tokens,
        "status": row.status,
        "createdAt": row.created_at,
    })
}
