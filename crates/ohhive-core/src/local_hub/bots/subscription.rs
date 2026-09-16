//! Atomic, fenced publication for completed subscription turns; trusted local API.
use super::*;
use crate::subscription::{
    journal::{Binding, Provider},
    results::CompletedReply,
};
type StoredReply = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);

impl LocalHubStore {
    pub(crate) fn bots_publish_subscription(
        &self,
        binding: &Binding,
        operation: Uuid,
        generation: u64,
        reply: CompletedReply,
    ) -> Result<MessageId> {
        let generation = as_i64(generation)?;
        let policy = binding
            .policy_revision
            .parse::<u32>()
            .map_err(|_| rejected("invalid subscription policy revision"))?;
        let runtime = match binding.provider {
            Provider::Copilot => "copilot_subscription",
            Provider::Chatgpt => "chatgpt_subscription",
            Provider::Grok => "grok_subscription",
        };
        let request = format!("delivery:{}:{}", operation, binding.agent);
        let source = format!(
            "subscription:{}:{}:{}",
            binding.session, operation, reply.receipt
        );
        self.transaction(|tx| {
            // Revalidate current profile, room, post permission and delivery inside
            // the same write transaction as publication; no permission-check race.
            let allowed:Option<String>=tx.query_row(
                "SELECT m.allowed_actions FROM conversation_members m JOIN conversations c ON c.id=m.conversation_id JOIN agent_profiles a ON a.id=m.principal_id WHERE c.id=?1 AND c.owner=?2 AND c.policy_revision=?3 AND m.principal_kind='agent' AND a.id=?4 AND a.owner=?2 AND a.archived=0 AND a.runtime_kind=?5 AND a.provider_account_ref=?6 AND a.preferred_host=?7",
                params![binding.conversation.to_string(),binding.owner.to_string(),policy,binding.agent.to_string(),runtime,binding.account.to_string(),binding.host.to_string()],|r|r.get(0)).optional().map_err(db_error)?;
            let actions:Vec<MemberAction>=decode(&allowed.ok_or_else(||rejected("forbidden: subscription binding no longer authorized"))?)?;
            if !actions.contains(&MemberAction::Post) {return Err(rejected("forbidden: agent cannot post"));}
            let row:Option<(String,i64,Option<String>)>=tx.query_row(
                "SELECT d.status,d.lease_generation,m.thread_root FROM agent_deliveries d JOIN messages m ON m.id=d.message_id WHERE d.message_id=?1 AND d.recipient=?2 AND m.conversation_id=?3",
                params![operation.to_string(),binding.agent.to_string(),binding.conversation.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
            let (status,current_generation,root)=row.ok_or_else(||rejected("not found: subscription delivery"))?;
            if current_generation!=generation || generation==0 {return Err(rejected("conflict: stale delivery generation"));}
            let root=root.unwrap_or_else(||operation.to_string());
            let existing:Option<StoredReply>=tx.query_row(
                "SELECT id,author_kind,author_id,body,turn_ref,source_event_ref,thread_root,kind FROM messages WHERE conversation_id=?1 AND client_request_id=?2",
                params![binding.conversation.to_string(),request],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional().map_err(db_error)?;
            if let Some((id,kind,author,body,turn,event,thread,message_kind))=existing {
                if status!="done" || kind!="agent" || author!=binding.agent.to_string() || body.as_deref()!=Some(&reply.text) || turn.as_deref()!=Some(&reply.turn) || event.as_deref()!=Some(&source) || thread.as_deref()!=Some(&root) || message_kind!="text" {
                    return Err(rejected("conflict: subscription reply differs from stored message"));
                }
                return Uuid::parse_str(&id).map_err(|_|rejected("invalid stored message ID"));
            }
            if status!="running" {return Err(rejected("conflict: subscription delivery is not running"));}
            let sequence:i64=tx.query_row("SELECT COALESCE(MAX(server_sequence),0)+1 FROM messages WHERE conversation_id=?1",params![binding.conversation.to_string()],|r|r.get(0)).map_err(db_error)?;
            let id=Uuid::new_v4();let ts=now();
            tx.execute("INSERT INTO messages(id,conversation_id,thread_root,author_kind,author_id,server_sequence,client_request_id,kind,body,attachment_refs,turn_ref,source_event_ref,created_at) VALUES(?1,?2,?3,'agent',?4,?5,?6,'text',?7,'[]',?8,?9,?10)",params![id.to_string(),binding.conversation.to_string(),root,binding.agent.to_string(),sequence,request,reply.text,reply.turn,source,ts]).map_err(db_error)?;
            tx.execute("UPDATE agent_deliveries SET status='done',retry_deadline=NULL,bound_runtime_session=?3,bound_turn_ref=?4,updated_at=?5 WHERE message_id=?1 AND recipient=?2",params![operation.to_string(),binding.agent.to_string(),binding.session.to_string(),reply.turn,ts]).map_err(db_error)?;
            Ok(id)
        })
    }
}
#[cfg(test)]
mod tests;
