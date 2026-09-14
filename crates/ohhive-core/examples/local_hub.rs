//! Developer entry point for ADR-025. Does not read community node.env or account settings.
use hive_core::{
    backend::{llama_cpp::LlamaCppBackend, Backend},
    capability::{Modality, ToolsLevel},
    hub::{ClaimedCard, Hub},
    local_hub::{serve, LocalHubStore, NodeCredentials, RemoteLocalHub},
    worker::Worker,
};
use std::{path::Path, time::Duration};
use uuid::Uuid;
fn arg(args: &[String], n: usize) -> anyhow::Result<&str> {
    args.get(n)
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing argument; run without arguments for usage"))
}
fn save(path: &str, creds: &NodeCredentials) -> anyhow::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(path)?;
    f.write_all(&serde_json::to_vec(creds)?)?;
    f.sync_all()?;
    println!("Paired node {}. Credentials saved locally.", creds.node_id);
    Ok(())
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(String::as_str){
  Some("init")=>{let store=LocalHubStore::open(arg(&a,1)?)?;save(arg(&a,2)?,&store.enroll_owner("hub machine")?)?;}
  Some("pair-code")=>{println!("{}",LocalHubStore::open(arg(&a,1)?)?.pairing_code()?);}
  Some("pair")=>{save(arg(&a,4)?,&RemoteLocalHub::pair(arg(&a,1)?,arg(&a,2)?,arg(&a,3)?).await?)?;}
  Some("project")=>{println!("{}",LocalHubStore::open(arg(&a,1)?)?.create_project(arg(&a,2)?,a.get(3).map(String::as_str).unwrap_or(""))?);}
  Some("card")=>{let card:ClaimedCard=serde_json::from_slice(&std::fs::read(arg(&a,2)?)?)?;println!("{}",LocalHubStore::open(arg(&a,1)?)?.add_card(card)?);}
  Some("inspect")=>{println!("{}",serde_json::to_string_pretty(&LocalHubStore::open(arg(&a,1)?)?.inspect()?)?);}
  Some("revoke")=>{LocalHubStore::open(arg(&a,1)?)?.revoke(arg(&a,2)?.parse::<Uuid>()?)?;}
  Some("serve")=>{let store=LocalHubStore::open(arg(&a,1)?)?;let address=a.get(2).map(String::as_str).unwrap_or("127.0.0.1:8787");let listener=tokio::net::TcpListener::bind(address).await?;println!("Local hub listening on {}",listener.local_addr()?);serve(store,listener,async{let _=tokio::signal::ctrl_c().await;}).await?;}
  Some("work")=>{
   let target=arg(&a,1)?;let credentials:NodeCredentials=serde_json::from_slice(&std::fs::read(arg(&a,2)?)?)?;
   let hub:Box<dyn Hub>=if target.starts_with("http://")||target.starts_with("https://"){Box::new(RemoteLocalHub::new(target,credentials.raw_key)?)}else{Box::new(LocalHubStore::open(target)?.connect(&credentials.raw_key)?)};
   let backend=LlamaCppBackend::new(arg(&a,3)?);let mut caps=backend.capabilities().await?;
   // This command is an explicit opt-in to the existing full-access private coding runtime.
   caps.modalities.push(Modality::Code);caps.tools_level=ToolsLevel::SandboxedTools;
   caps.allow_internet=a.iter().any(|s|s=="--allow-internet");
   let data=Path::new(arg(&a,4)?).to_path_buf();std::fs::create_dir_all(&data)?;
   hub.check_in(&caps,None).await?;let(stop,rx)=tokio::sync::watch::channel(false);
   tokio::spawn(async move{let _=tokio::signal::ctrl_c().await;let _=stop.send(true);});
   let worker=Worker{hub:hub.as_ref(),backend:&backend,caps:&caps,default_model:caps.models.first().map(|m|m.id.clone()),stop:rx,events:None,data_dir:data,sandbox:None};
   worker.run_forever(Duration::from_secs(2),10).await?;
  }
  _=>println!("Fully local hub developer commands:\n  init <db> <new-owner-credentials-file>\n  pair-code <db>\n  pair <hub-origin> <code> <node-name> <new-credentials-file>\n  project <db> <title> [goal]\n  card <db> <card-json-file>\n  inspect <db>\n  revoke <db> <node-id>\n  serve <db> [private-bind-address:port]\n  work <db-or-hub-origin> <credentials-file> <local-model-origin> <scratch-dir> [--allow-internet]\nNo account login or Supabase configuration is used. On Windows, put the DB and credentials in a user-private folder. Plain HTTP LAN pairing assumes a trusted private network; use an owner-managed HTTPS endpoint for other networks."),
 };
    Ok(())
}
