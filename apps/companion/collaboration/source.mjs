import { connector } from '../connector/stdio.mjs';
/** Uses the same scoped project service as the companion; no second conversation log. */
export async function projectSource(config, fetcher=fetch) {
  const handle=connector(config,fetcher);let id=0;
  const initialized=await handle({jsonrpc:'2.0',id:++id,method:'initialize',params:{protocolVersion:'2025-06-18',capabilities:{},clientInfo:{name:'den_watcher',version:'0.1.0'}}});
  if(initialized.error)throw new Error('Project connection rejected.');
  async function call(name,args){
    const r=await handle({jsonrpc:'2.0',id:++id,method:'tools/call',params:{name,arguments:args}});
    if(r.error||r.result?.isError)throw new Error('Project access rejected.');
    return JSON.parse(r.result.content[0].text);
  }
  return {identity:()=>call('list_project_rooms',{}),read:(room,after)=>call('read_project_updates',{room_id:room,after_sequence:after,limit:100}),
    async message(room,seq){const p=await this.read(room,seq-1);return p.updates.find(u=>u.sequence===seq);},
    async post(room,request,body){const p=await this.read(room,0);return call('post_project_update',{room_id:room,request_id:request,policy_revision:p.room.policy_revision,body});}
  };
}
