//! Trusted local administration for the first project connector. No node key is exported.
use hive_core::local_hub::LocalHubStore;
use std::io::Write;
fn arg(a: &[String], i: usize) -> anyhow::Result<&str> {
    a.get(i)
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing argument"))
}
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(String::as_str){
        Some("grant")=>{
            if a.len()!=8{anyhow::bail!("grant requires database, owner, agent, room, read|post, TTL seconds and a NEW credential file");}
            let can_post=match arg(&a,5)?{"read"=>false,"post"=>true,_=>anyhow::bail!("permission must be read or post")};
            let owner=arg(&a,2)?.parse()?;
            let agent=if arg(&a,3)?=="human" {None}else{Some(arg(&a,3)?.parse()?)};
            let room=arg(&a,4)?.parse()?;
            let ttl=arg(&a,6)?.parse()?;
            let store=LocalHubStore::open(arg(&a,1)?)?;
            // Reserve a protected file before issuing a credential. Existing files are never overwritten.
            let mut opts=std::fs::OpenOptions::new();opts.write(true).create_new(true);
            #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt; opts.mode(0o600);}
            let path=arg(&a,7)?;
            let mut file=opts.open(path)?;
            let result=match agent {Some(agent)=>store.project_connector_grant(owner,agent,vec![room],can_post,ttl),None=>store.project_connector_human_grant(owner,vec![room],can_post,ttl)};
            let g=match result {
                Ok(g)=>g,
                Err(e)=>{drop(file);let _=std::fs::remove_file(path);return Err(e.into());}
            };
            if let Err(e)=file.write_all(&serde_json::to_vec(&g)?).and_then(|_|file.sync_all()) {
                store.project_connector_revoke(owner,g.grant_id)?;
                drop(file);let _=std::fs::remove_file(path);return Err(e.into());
            }
            println!("Grant {} written to protected file; expires at {}. No task execution enabled.",g.grant_id,g.expires_at);
        },
        Some("revoke")=>{
            if a.len()!=4{anyhow::bail!("revoke requires database, owner and grant identifier");}
            LocalHubStore::open(arg(&a,1)?)?.project_connector_revoke(arg(&a,2)?.parse()?,arg(&a,3)?.parse()?)?;
            println!("Project connector grant revoked.");
        },
        _=>println!("Trusted LOCAL administration:\n  grant <database> <owner-id> <existing-agent-id|human> <project-room-id> <read|post> <TTL-seconds> <new-private-file>\n  revoke <database> <owner-id> <grant-id>\nNever put credential files in a repository, Library collection, shared folder or logs.")
    }
    Ok(())
}
