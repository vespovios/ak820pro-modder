import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Card, ErrorBanner } from "../components/ui";
import { formatError } from "../errors";

type AppIdentity = {bundle_id:string;name:string};
type Rule = {app:AppIdentity;setup_id:number};
type Settings = {enabled:boolean;fallback_id:number|null;rules:Rule[]};
type Status = {settings:Settings;active_app:AppIdentity|null;running:boolean;last_setup:string|null;error:string|null};
const selectStyle="h-9 rounded-sm border border-line bg-surface-base px-2 text-fg-0";
export function AppSetupRules({profiles}:{profiles:{id:number;name:string}[]}) {
  const [settings,setSettings]=useState<Settings>({enabled:false,fallback_id:null,rules:[]});
  const [status,setStatus]=useState<Status|null>(null);
  const [apps,setApps]=useState<AppIdentity[]>([]);
  const [appId,setAppId]=useState("");
  const [setupId,setSetupId]=useState("");
  const [busy,setBusy]=useState(false);
  const [dirty,setDirty]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const [receipt,setReceipt]=useState<string|null>(null);
  useEffect(()=>{
    let alive=true;
    void Promise.all([invoke<Status>("app_setup_status"),invoke<AppIdentity[]>("list_setup_apps")]).then(([s,a])=>{if(alive){setStatus(s);setSettings(s.settings);setApps(a);}}).catch(e=>{if(alive)setError(formatError(e));});
    const timer=window.setInterval(()=>void invoke<Status>("app_setup_status").then(s=>{if(alive)setStatus(s);}).catch(()=>{}),1000);
    return()=>{alive=false;window.clearInterval(timer);};
  },[]);
  function edit(next:Settings){setSettings(next);setDirty(true);setReceipt(null);}
  async function save(next:Settings){
    setBusy(true);setError(null);setReceipt(null);
    try{await invoke("save_app_setup_rules",{settings:next});setSettings(next);setDirty(false);setStatus(await invoke("app_setup_status"));setReceipt(next.enabled?"Automatic switching enabled. Switch to an assigned app to try it.":"Automatic switching paused.");}
    catch(e){setError(formatError(e));}finally{setBusy(false);}
  }
  async function refresh(){setBusy(true);setError(null);try{setApps(await invoke("list_setup_apps"));}catch(e){setError(formatError(e));}finally{setBusy(false);}}
  const available=apps.filter(a=>!settings.rules.some(r=>r.app.bundle_id===a.bundle_id));
  return <Card title="Switch setups by app">
    <div className="space-y-4 text-sm text-fg-2">
      <p>Choose a setup for each app. Switching happens after the app stays active for about a second. Keep AK820 Pro Modder open; its own window never triggers a switch.</p>
      <ErrorBanner>{error||status?.error}</ErrorBanner>
      <p role="status">{status?.settings.enabled?"Enabled":"Paused"}{status?.active_app?` · Active app: ${status.active_app.name}`:""}{status?.running?" · Applying setup…":status?.last_setup?` · Last automatic setup: ${status.last_setup}`:""}</p>
      {settings.rules.map((rule,index)=><div key={rule.app.bundle_id} className="flex flex-wrap items-end gap-2 rounded-sm border border-line p-3">
        <div className="min-w-40 flex-1"><p className="font-medium text-fg-0">{rule.app.name}</p><p className="break-all text-xs text-fg-3">{rule.app.bundle_id}</p></div>
        <label className="grid flex-1 gap-1">Setup for {rule.app.name}<select aria-label={`Setup for ${rule.app.name}`} className={selectStyle} value={rule.setup_id} disabled={busy} onChange={e=>edit({...settings,rules:settings.rules.map((r,i)=>i===index?{...r,setup_id:Number(e.target.value)}:r)})}>
          {!profiles.some(p=>p.id===rule.setup_id)&&<option value={rule.setup_id}>Deleted setup — choose another</option>}{profiles.map(p=><option key={p.id} value={p.id}>{p.name}</option>)}
        </select></label>
        <Button size="sm" disabled={busy} onClick={()=>edit({...settings,rules:settings.rules.filter((_,i)=>i!==index)})}>Remove {rule.app.name} rule</Button>
      </div>)}
      <div className="flex flex-wrap items-end gap-2">
        <label className="grid min-w-40 flex-1 gap-1">Application<select aria-label="Application for setup rule" className={selectStyle} value={appId} disabled={busy} onChange={e=>setAppId(e.target.value)}><option value="">Choose an open app…</option>{available.map(a=><option key={a.bundle_id} value={a.bundle_id}>{a.name}</option>)}</select></label>
        <label className="grid min-w-40 flex-1 gap-1">Automatic setup<select aria-label="New app rule setup" className={selectStyle} value={setupId} disabled={busy} onChange={e=>setSetupId(e.target.value)}><option value="">Choose a saved setup…</option>{profiles.map(p=><option key={p.id} value={p.id}>{p.name}</option>)}</select></label>
        <Button size="sm" disabled={busy||!available.some(a=>a.bundle_id===appId)||!profiles.some(p=>p.id===Number(setupId))} onClick={()=>{const app=available.find(a=>a.bundle_id===appId);if(app){edit({...settings,rules:[...settings.rules,{app,setup_id:Number(setupId)}]});setAppId("");setSetupId("");}}}>Add app rule</Button>
        <Button size="sm" disabled={busy} onClick={()=>void refresh()}>Refresh open apps</Button>
      </div>
      <p className="text-xs text-fg-3">Open an app and refresh this list to add it. Rules continue to work when that app is reopened.</p>
      <label className="grid max-w-sm gap-1">For other apps<select aria-label="Fallback keyboard setup" className={selectStyle} disabled={busy} value={settings.fallback_id??""} onChange={e=>edit({...settings,fallback_id:e.target.value===""?null:Number(e.target.value)})}><option value="">Keep current keyboard settings</option>{settings.fallback_id!==null&&!profiles.some(p=>p.id===settings.fallback_id)&&<option value={settings.fallback_id}>Deleted setup — choose another</option>}{profiles.map(p=><option key={p.id} value={p.id}>{p.name}</option>)}</select></label>
      <div className="flex flex-wrap gap-2"><Button size="sm" variant="primary" disabled={busy||!profiles.length} onClick={()=>void save({...settings,enabled:true})}>Save rules & enable</Button><Button size="sm" disabled={busy||(!status?.settings.enabled&&!dirty)} onClick={()=>void save({...settings,enabled:false})}>Save rules & pause</Button></div>
      {dirty&&<p>Unsaved rule changes. Saved rules continue until you save.</p>}
      {receipt&&<p role="status" className="text-good">{receipt}</p>}
      <p className="text-xs text-fg-3">Complete setups replace their included settings and Mac remap/macro library. Macros are loaded without playing. Manual changes stay until the next app switch. A failed upload stops and is retried on a later activation, or after saving rules again. Only app names and identifiers are observed; no window content is read.</p>
    </div>
  </Card>;
}
