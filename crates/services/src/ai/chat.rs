//! AI assistant: conversations, messages, and the RBAC-aware tool-calling loop.
//!
//! The assistant persists conversation history, calls the configured provider,
//! and executes typed tool requests under the current user's RBAC. Mutating
//! tools are not executed automatically — they are returned as a
//! `confirmationRequired` payload that the client must confirm via
//! `confirm_tool_calls`.

use ai::{AiMessage, AiRequest, AiTool, AiToolCall};
use db::entities::{ai_conversation, ai_message, ai_model, ai_provider};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set,
};
use serde_json::{json, Value};

use crate::ai::providers::build_provider;
use crate::ai::security::requires_confirmation;
use crate::ai::tools::{definitions, execute_tool};
use crate::ai::usage::log_usage;
use crate::{AppContext, ServiceError};

const MAX_TOOL_ITERATIONS: usize = 5;

// ---------------------------------------------------------------------------
// Conversations
// ---------------------------------------------------------------------------

pub async fn create_conversation(
    ctx: &AppContext,
    title: String,
    system_prompt: Option<String>,
    provider_id: Option<i64>,
    model: Option<String>,
    privacy_mode: bool,
) -> Result<serde_json::Value, ServiceError> {
    let user = ctx.require_admin()?;
    let now = chrono::Utc::now();
    let row = ai_conversation::ActiveModel {
        user_id: Set(user.id),
        provider_id: Set(provider_id),
        model: Set(model),
        title: Set(title),
        system_prompt: Set(system_prompt),
        requires_confirmation: Set(true),
        privacy_mode: Set(privacy_mode),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    Ok(conversation_dto(row))
}

pub async fn list_conversations(ctx: &AppContext) -> Result<Vec<serde_json::Value>, ServiceError> {
    let user = ctx.require_admin()?;
    let rows = ai_conversation::Entity::find()
        .filter(ai_conversation::Column::UserId.eq(user.id))
        .order_by_desc(ai_conversation::Column::UpdatedAt)
        .all(&ctx.db)
        .await?;
    Ok(rows.into_iter().map(conversation_dto).collect())
}

pub async fn get_conversation(ctx: &AppContext, id: i64) -> Result<serde_json::Value, ServiceError> {
    let user = ctx.require_admin()?;
    let row = ai_conversation::Entity::find_by_id(id)
        .filter(ai_conversation::Column::UserId.eq(user.id))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("ai conversation {id}")))?;
    Ok(conversation_dto(row))
}

pub async fn delete_conversation(ctx: &AppContext, id: i64) -> Result<(), ServiceError> {
    let user = ctx.require_admin()?;
    let row = ai_conversation::Entity::find_by_id(id)
        .filter(ai_conversation::Column::UserId.eq(user.id))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("ai conversation {id}")))?;
    ai_message::Entity::delete_many()
        .filter(ai_message::Column::ConversationId.eq(id))
        .exec(&ctx.db)
        .await?;
    let am: ai_conversation::ActiveModel = row.into();
    am.delete(&ctx.db).await?;
    Ok(())
}

fn conversation_dto(row: ai_conversation::Model) -> serde_json::Value {
    json!({
        "id": row.id,
        "title": row.title,
        "providerId": row.provider_id,
        "model": row.model,
        "systemPrompt": row.system_prompt,
        "requiresConfirmation": row.requires_confirmation,
        "privacyMode": row.privacy_mode,
        "createdAt": row.created_at,
        "updatedAt": row.updated_at,
    })
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

pub async fn list_messages(ctx: &AppContext, conversation_id: i64) -> Result<Vec<serde_json::Value>, ServiceError> {
    let user = ctx.require_admin()?;
    // Ownership check.
    ai_conversation::Entity::find_by_id(conversation_id)
        .filter(ai_conversation::Column::UserId.eq(user.id))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("ai conversation {conversation_id}")))?;
    let rows = ai_message::Entity::find()
        .filter(ai_message::Column::ConversationId.eq(conversation_id))
        .order_by_asc(ai_message::Column::Id)
        .all(&ctx.db)
        .await?;
    Ok(rows.into_iter().map(message_dto).collect())
}

fn message_dto(row: ai_message::Model) -> serde_json::Value {
    json!({
        "id": row.id,
        "conversationId": row.conversation_id,
        "role": row.role,
        "content": row.content,
        "toolCalls": row.tool_calls_json,
        "toolCallId": row.tool_call_id,
        "toolName": row.tool_name,
        "inputTokens": row.input_tokens,
        "outputTokens": row.output_tokens,
        "createdAt": row.created_at,
    })
}

async fn insert_message(
    ctx: &AppContext,
    conversation_id: i64,
    role: &str,
    content: &str,
    tool_calls: Option<&[AiToolCall]>,
    tool_call_id: Option<&str>,
    tool_name: Option<&str>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
) -> Result<(), ServiceError> {
    let tool_calls_json = tool_calls.map(|c| serde_json::to_value(c.to_vec()).unwrap_or(Value::Null));
    let now = chrono::Utc::now();
    ai_message::ActiveModel {
        conversation_id: Set(conversation_id),
        role: Set(role.to_string()),
        content: Set(content.to_string()),
        tool_calls_json: Set(tool_calls_json),
        tool_call_id: Set(tool_call_id.map(|s| s.to_string())),
        tool_name: Set(tool_name.map(|s| s.to_string())),
        input_tokens: Set(input_tokens),
        output_tokens: Set(output_tokens),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Assistant
// ---------------------------------------------------------------------------

/// Build the provider message history from a conversation's persisted messages.
fn history_messages(rows: &[ai_message::Model]) -> Vec<AiMessage> {
    rows.iter()
        .map(|m| {
            let tool_calls = m
                .tool_calls_json
                .as_ref()
                .and_then(|j| serde_json::from_value::<Vec<AiToolCall>>(j.clone()).ok());
            AiMessage {
                role: match m.role.as_str() {
                    "assistant" => ai::AiMessageRole::Assistant,
                    "tool" => ai::AiMessageRole::Tool,
                    _ => ai::AiMessageRole::User,
                },
                content: m.content.clone(),
                tool_calls,
                tool_call_id: m.tool_call_id.clone(),
                name: m.tool_name.clone(),
            }
        })
        .collect()
}

/// Send a user message to the assistant and run the tool-calling loop.
///
/// Non-mutating tools execute immediately under RBAC. Mutating tools are NOT
/// executed — they are returned as `confirmationRequired` for the client to
/// approve via `confirm_tool_calls`.
// ---------------------------------------------------------------------------
// Provider resolution
// ---------------------------------------------------------------------------

/// Resolve the provider + model for a conversation.
///
/// A conversation may be created with only a model name (no provider), so the
/// provider is resolved from that model; otherwise it falls back to the first
/// enabled provider that has an enabled chat model.
async fn resolve_provider_and_model(
    ctx: &AppContext,
    conv: &ai_conversation::Model,
) -> Result<(i64, String), ServiceError> {
    // 1. Prefer an explicit provider; otherwise find the provider that owns a
    //    model matching the conversation's model name.
    let provider_id = if let Some(pid) = conv.provider_id {
        Some(pid)
    } else if let Some(name) = &conv.model {
        ai_model::Entity::find()
            .filter(ai_model::Column::Name.eq(name.clone()))
            .filter(ai_model::Column::SupportsChat.eq(true))
            .filter(ai_model::Column::Enabled.eq(true))
            .one(&ctx.db)
            .await?
            .map(|m| m.provider_id)
    } else {
        None
    };

    let provider_id = match provider_id {
        Some(pid) => pid,
        None => {
            // Fall back to the first enabled provider with a chat model.
            let m = ai_model::Entity::find()
                .filter(ai_model::Column::SupportsChat.eq(true))
                .filter(ai_model::Column::Enabled.eq(true))
                .order_by_asc(ai_model::Column::Id)
                .one(&ctx.db)
                .await?
                .ok_or_else(|| {
                    ServiceError::internal(
                        "no AI provider is configured — add a provider and model in AI Settings first",
                    )
                })?;
            m.provider_id
        }
    };

    // Verify the provider still exists and is enabled.
    let prow = ai_provider::Entity::find_by_id(provider_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::internal("AI provider no longer exists"))?;
    if !prow.enabled {
        return Err(ServiceError::internal(format!(
            "AI provider '{}' is disabled — enable it in AI Settings",
            prow.name
        )));
    }

    // 2. Resolve the model name (explicit, else the provider's first chat model).
    let model = if let Some(name) = &conv.model {
        name.clone()
    } else {
        ai_model::Entity::find()
            .filter(ai_model::Column::ProviderId.eq(provider_id))
            .filter(ai_model::Column::SupportsChat.eq(true))
            .filter(ai_model::Column::Enabled.eq(true))
            .order_by_asc(ai_model::Column::Id)
            .one(&ctx.db)
            .await?
            .map(|m| m.name)
            .ok_or_else(|| {
                ServiceError::internal(format!(
                    "AI provider '{}' has no chat model configured",
                    prow.name
                ))
            })?
    };

    Ok((provider_id, model))
}

pub async fn send_message(
    ctx: &AppContext,
    conversation_id: i64,
    text: &str,
) -> Result<serde_json::Value, ServiceError> {
    let user = ctx.require_admin()?;
    let conv = ai_conversation::Entity::find_by_id(conversation_id)
        .filter(ai_conversation::Column::UserId.eq(user.id))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("ai conversation {conversation_id}")))?;
    let (provider_id, model) = resolve_provider_and_model(ctx, &conv).await?;
    let (_prow, provider) = build_provider(ctx, provider_id).await?;

    insert_message(ctx, conversation_id, "user", text, None, None, None, None, None).await?;

    let prior = ai_message::Entity::find()
        .filter(ai_message::Column::ConversationId.eq(conversation_id))
        .order_by_asc(ai_message::Column::Id)
        .all(&ctx.db)
        .await?;
    // Privacy mode: never send conversation history to the provider — only the
    // current user message (plus system prompt) leaves the CMS.
    let mut messages = if conv.privacy_mode {
        prior
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .cloned()
            .map(|m| history_messages(std::slice::from_ref(&m)))
            .unwrap_or_default()
    } else {
        history_messages(&prior)
    };

    let tools: Vec<AiTool> = definitions();
    let mut total_in = 0u64;
    let mut total_out = 0u64;
    let mut pending_confirmation: Option<Vec<AiToolCall>> = None;
    let mut executed: Vec<Value> = Vec::new();

    for _ in 0..MAX_TOOL_ITERATIONS {
        let request = AiRequest {
            model: model.clone(),
            messages: messages.clone(),
            system: conv.system_prompt.clone(),
            temperature: Some(0.6),
            max_tokens: Some(4000),
            tools: Some(tools.clone()),
        };
        let resp = provider.chat(&request).await.map_err(|e| ServiceError::internal(e.to_string()))?;
        total_in += resp.usage.input_tokens;
        total_out += resp.usage.output_tokens;

        let calls = resp.tool_calls.clone().unwrap_or_default();
        if calls.is_empty() {
            // A provider can return an empty completion (e.g. it burned its
            // whole output budget on hidden reasoning). Don't store an empty
            // assistant bubble — surface a clear message instead.
            if resp.content.trim().is_empty() {
                return Err(ServiceError::internal(
                    "the AI provider returned an empty response (it may have hit its token limit). \
                     Try rephrasing your request or using a model with a larger output window.",
                ));
            }
            insert_message(
                ctx, conversation_id, "assistant", &resp.content, None, None, None,
                Some(resp.usage.input_tokens as i64), Some(resp.usage.output_tokens as i64),
            ).await?;
            let _ = log_usage(ctx, user.id, Some(provider_id), Some(&model), Some("chat"), resp.usage, Some("ok")).await;
            return Ok(json!({
                "content": resp.content,
                "executedTools": executed,
                "confirmationRequired": null,
                "usage": { "input": total_in, "output": total_out },
            }));
        }

        // Assistant message requesting tools.
        insert_message(
            ctx, conversation_id, "assistant", "", Some(&calls), None, None,
            Some(resp.usage.input_tokens as i64), Some(resp.usage.output_tokens as i64),
        ).await?;

        // Separate mutating from safe calls without moving `calls`.
        let mut safe: Vec<AiToolCall> = Vec::new();
        let mut mutating: Vec<AiToolCall> = Vec::new();
        for c in &calls {
            if requires_confirmation(&c.name) {
                mutating.push(c.clone());
            } else {
                safe.push(c.clone());
            }
        }
        if !mutating.is_empty() {
            // Persist tool result placeholders so history stays coherent; the
            // pending mutating calls await confirmation.
            for c in &safe {
                let r = execute_tool(ctx, c, Some(&model)).await?;
                insert_message(ctx, conversation_id, "tool", &r.content, None, Some(&r.call_id), Some(&r.name), None, None).await?;
                executed.push(json!({ "name": r.name, "ok": true }));
            }
            for c in &mutating {
                executed.push(json!({ "name": c.name, "pendingConfirmation": true }));
            }
            pending_confirmation = Some(mutating);
            break;
        }

        // Execute all safe tool calls and feed results back.
        messages.push(AiMessage::assistant_tool_calls(calls.clone()));
        for c in &calls {
            let r = execute_tool(ctx, c, Some(&model)).await?;
            insert_message(ctx, conversation_id, "tool", &r.content, None, Some(&r.call_id), Some(&r.name), None, None).await?;
            messages.push(AiMessage::tool(&r.name, &r.call_id, &r.content));
            executed.push(json!({ "name": r.name }));
        }
    }

    // Loop exhausted or confirmation required.
    if let Some(pending) = pending_confirmation {
        return Ok(json!({
            "content": "Some actions need your confirmation before they run.",
            "executedTools": executed,
            "confirmationRequired": pending,
            "usage": { "input": total_in, "output": total_out },
        }));
    }
    Err(ServiceError::internal("assistant hit the tool-call limit"))
}

/// Execute previously-confirmed mutating tool calls, then let the model produce
/// a final answer.
pub async fn confirm_tool_calls(
    ctx: &AppContext,
    conversation_id: i64,
    calls: Vec<AiToolCall>,
) -> Result<serde_json::Value, ServiceError> {
    let user = ctx.require_admin()?;
    let conv = ai_conversation::Entity::find_by_id(conversation_id)
        .filter(ai_conversation::Column::UserId.eq(user.id))
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("ai conversation {conversation_id}")))?;
    let (provider_id, model) = resolve_provider_and_model(ctx, &conv).await?;
    let (_prow, provider) = build_provider(ctx, provider_id).await?;

    let mut executed: Vec<Value> = Vec::new();
    let mut messages = Vec::<AiMessage>::new();
    for c in &calls {
        let r = execute_tool(ctx, c, Some(&model)).await?;
        insert_message(ctx, conversation_id, "tool", &r.content, None, Some(&r.call_id), Some(&r.name), None, None).await?;
        executed.push(json!({ "name": r.name, "ok": r.content.contains("\"ok\":true") || !r.content.contains("\"error\"") }));
    }

    // Rebuild full history (now includes the user msg, assistant tool-call msg,
    // and the tool results) and ask the model for a final answer.
    let prior = ai_message::Entity::find()
        .filter(ai_message::Column::ConversationId.eq(conversation_id))
        .order_by_asc(ai_message::Column::Id)
        .all(&ctx.db)
        .await?;
    messages = history_messages(&prior);

    let request = AiRequest {
        model: model.clone(),
        messages,
        system: conv.system_prompt.clone(),
        temperature: Some(0.5),
        max_tokens: Some(4000),
        tools: None,
    };
    let resp = provider.chat(&request).await.map_err(|e| ServiceError::internal(e.to_string()))?;
    insert_message(
        ctx, conversation_id, "assistant", &resp.content, None, None, None,
        Some(resp.usage.input_tokens as i64), Some(resp.usage.output_tokens as i64),
    ).await?;
    let _ = log_usage(ctx, user.id, Some(provider_id), Some(&model), Some("chat.confirm"), resp.usage, Some("ok")).await;
    Ok(json!({
        "content": resp.content,
        "executedTools": executed,
        "confirmationRequired": null,
        "usage": { "input": resp.usage.input_tokens, "output": resp.usage.output_tokens },
    }))
}
