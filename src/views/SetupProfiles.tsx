import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Card, ErrorBanner } from "../components/ui";
import { PageHeader } from "../components/Layout";
import { type ColourProfile, useColourEvent } from "../colour-profile-events";
import type { LightingConfig } from "../types";
import { formatError } from "../errors";
import { renderFrame, type Frame } from "./TftDisplay";
import { type HostRemap } from "./HostKeymap";
import { AppSetupRules } from "./AppSetupRules";
import { hidName } from "../data/hid-usage-names";

export interface SetupSnapshot {
  lighting: {kind:"global";config:LightingConfig}|{kind:"per_key";profile:ColourProfile}|null;
  sleep_minutes:number|null;
  host:{enabled:boolean;remaps:HostRemap[];macros:{macro_id:number;name:string|null;actions:{key_code:number;is_press:boolean;delay_ms:number;kind:string}[]}[];markers:Record<string,number>};
  display:{name:string;animation:{frames:Frame[]}}|null;
}
interface NamedSetup {id:number;name:string;snapshot:SetupSnapshot}
export interface SetupReport {id:number;name:string;success:boolean;completed:string[];failed:string|null;error:string|null;warnings:string[]}
interface ShortcutStatus {settings:{enabled:boolean;shortcut:string};running:boolean;cursor:number|null;error:string|null}

export function SetupProfiles({legacy}:{legacy:boolean}) {
  const [profiles,setProfiles]=useState<NamedSetup[]>([]);
  const [colours,setColours]=useState<{id:number;name:string;profile:ColourProfile}[]>([]);
  const [draft,setDraft]=useState<SetupSnapshot|null>(null);
  const [captured,setCaptured]=useState<SetupSnapshot|null>(null);
  const [selectedId,setSelectedId]=useState<number|null>(null);
  const [name,setName]=useState("");
  const [edited,setEdited]=useState(false);
  const [working,setWorking]=useState(false);
  const [applying,setApplying]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const [receipt,setReceipt]=useState<string|null>(null);
  const [report,setReport]=useState<SetupReport|null>(null);
  const [progress,setProgress]=useState<string|null>(null);
  const [confirmDelete,setConfirmDelete]=useState(false);
  const [shortcut,setShortcut]=useState("Control+Alt+Super+KeyS");
  const [shortcutEnabled,setShortcutEnabled]=useState(false);
  const [recording,setRecording]=useState(false);
  const input=useRef<HTMLInputElement>(null);
  const preview=useRef<HTMLCanvasElement>(null);
  const selected=profiles.find(p=>p.id===selectedId);
  const locked=working||applying;
  const validName=!!name.trim()&&[...name.trim()].length<=64;

  useEffect(()=>{
    if(!legacy)return;
    let alive=true;
    setWorking(true);
    void Promise.all([
      invoke<NamedSetup[]>("list_setup_profiles"),invoke<SetupSnapshot>("get_setup_snapshot"),
      invoke<typeof colours>("list_feature_rgb_profiles"),invoke<ShortcutStatus>("setup_shortcut_status"),
    ]).then(([list,snapshot,colours,status])=>{
      if(!alive)return;
      setProfiles(list);setDraft(snapshot);setCaptured(snapshot);setColours(colours);
      setShortcut(status.settings.shortcut);setShortcutEnabled(status.settings.enabled);setApplying(status.running);
      if(status.error)setError(status.error);
    }).catch(e=>{if(alive)setError(formatError(e));}).finally(()=>{if(alive)setWorking(false);});
    const timer=window.setInterval(()=>void invoke<ShortcutStatus>("setup_shortcut_status").then(s=>{if(alive)setApplying(s.running);}).catch(()=>{}),2000);
    return()=>{alive=false;window.clearInterval(timer);};
  },[legacy]);

  useColourEvent<{name:string;component:string}>("setup-progress",p=>{setApplying(true);setProgress(`${p.name}: applying ${p.component}…`);setError(null);});
  useColourEvent<{completed:number;total:number}>("setup-tft-progress",p=>setProgress(`TFT display: ${p.completed} / ${p.total} blocks acknowledged`));
  useColourEvent<SetupReport>("setup-applied",r=>{
    setApplying(false);setProgress(null);setReport(r);
    if(r.success){setSelectedId(r.id);setName(r.name);setEdited(false);const p=profiles.find(p=>p.id===r.id);if(p)setDraft(p.snapshot);}
  });
  useColourEvent<string>("setup-switch-error",e=>{setApplying(false);setProgress(null);setError(e);});

  useEffect(()=>{
    const frames=draft?.display?.animation.frames;
    if(!frames?.length || !preview.current)return;
    let i=0;let timer:number|undefined;
    function show(){if(!preview.current||!frames)return;renderFrame(preview.current,frames[i]);const delay=frames[i].delay_ms;i=(i+1)%frames.length;if(frames.length>1)timer=window.setTimeout(show,delay);}
    show();return()=>window.clearTimeout(timer);
  },[draft?.display]);

  function edit(change:Partial<SetupSnapshot>){if(draft){setDraft({...draft,...change});setEdited(true);setReceipt(null);setReport(null);}}
  async function capture(){
    setWorking(true);setError(null);setReceipt(null);
    try{const snapshot=await invoke<SetupSnapshot>("get_setup_snapshot");setCaptured(snapshot);setDraft(snapshot);setEdited(true);setReport(null);setColours(await invoke("list_feature_rgb_profiles"));}
    catch(e){setError(formatError(e));}finally{setWorking(false);}
  }
  async function save(id:number|null){
    if(!draft)return;setWorking(true);setError(null);setReceipt(null);
    try{const saved=await invoke<NamedSetup>("save_setup_profile",{id,name,snapshot:draft});setProfiles(p=>id===null?[...p,saved]:p.map(v=>v.id===id?saved:v));setSelectedId(saved.id);setName(saved.name);setDraft(saved.snapshot);setEdited(false);setReceipt(`Saved “${saved.name}” on this Mac. The keyboard has not changed.`);}
    catch(e){setError(formatError(e));}finally{setWorking(false);}
  }
  async function apply(){
    if(!selected)return;setApplying(true);setError(null);setReport(null);setReceipt(null);
    try{setReport(await invoke<SetupReport>("apply_setup_profile",{id:selected.id}));}
    catch(e){setError(formatError(e));}finally{setApplying(false);setProgress(null);}
  }
  async function remove(){
    if(!selected)return;setWorking(true);setError(null);
    try{await invoke("delete_setup_profile",{id:selected.id});setProfiles(p=>p.filter(v=>v.id!==selected.id));setSelectedId(null);setName("");setConfirmDelete(false);setReceipt("Setup deleted. Current keyboard settings are unchanged.");}
    catch(e){setError(formatError(e));}finally{setWorking(false);}
  }
  async function loadScreen(file:File){
    setWorking(true);setError(null);
    try{if(file.size>10*1024*1024)throw new Error("Choose an image no larger than 10 MB.");const result=await invoke<{animation:{frames:Frame[]};timing_adjusted:boolean}>("prepare_tft_image",{bytes:Array.from(new Uint8Array(await file.arrayBuffer()))});edit({display:{name:[...file.name].slice(0,128).join(""),animation:result.animation}});if(result.timing_adjusted)setReceipt("GIF timing adjusted to the keyboard’s supported frame timing.");}
    catch(e){setError(formatError(e));}finally{setWorking(false);}
  }
  async function files(action:"import"|"export",all=false){
    setWorking(true);setError(null);setReceipt(null);
    try{
      if(action==="import"){
        const added=await invoke<NamedSetup[]|null>("import_setup_profiles");
        if(added?.length){setProfiles(await invoke("list_setup_profiles"));const p=added[0];setSelectedId(p.id);setName(p.name);setDraft(p.snapshot);setEdited(false);setReceipt(`Imported ${added.length} setup(s). Review the components before applying.`);}
      }else{const path=await invoke<string|null>("export_setup_profiles",{id:all?null:selectedId});if(path)setReceipt(`Exported to ${path}`);}
    }catch(e){setError(formatError(e));}finally{setWorking(false);}
  }
  async function saveShortcut(enabled:boolean){
    setWorking(true);setError(null);
    try{await invoke("save_setup_shortcut",{settings:{enabled,shortcut}});setShortcutEnabled(enabled);setReceipt(enabled?"Complete-setup shortcut enabled. Keep the app open.":"Complete-setup shortcut disabled.");}
    catch(e){setError(formatError(e));}finally{setWorking(false);}
  }

  if(!legacy)return <Card title="Setup profiles"><p className="text-sm text-fg-2">Connect your ANSI AK820 Pro to manage complete setups.</p></Card>;
  return <div className="space-y-6">
    <PageHeader title="Setup profiles" description="Save lighting, sleep, Mac key remaps, macros and a TFT image or GIF together." />
    <ErrorBanner>{error}</ErrorBanner>
    {progress&&<p role="status" className="text-sm text-fg-1">{progress}</p>}
    {report&&<div role="status" className={`rounded-md border p-4 text-sm ${report.success?"border-good/40 text-good":"border-bad/40 text-bad"}`}>
      <p>{report.success?`“${report.name}” applied.`:`“${report.name}” stopped at ${report.failed}.`}</p>
      <p>Completed: {report.completed.join(", ")||"none"}.</p>
      {report.error&&<p>{report.error} Earlier completed components remain applied.</p>}
      {report.warnings.map(w=><p key={w}>{w}</p>)}
    </div>}
    {receipt&&<p role="status" className="text-sm text-good">{receipt}</p>}
    <Card title="Saved setups">
      <div className="flex flex-wrap items-end gap-2">
        <label className="grid min-w-48 flex-1 gap-1 text-sm text-fg-2">Saved setup
          <select aria-label="Saved keyboard setup" disabled={locked} value={selectedId??""} onChange={e=>{const p=profiles.find(p=>p.id===Number(e.target.value));setSelectedId(p?.id??null);setName(p?.name??"");if(p)setDraft(p.snapshot);setEdited(false);setReport(null);setConfirmDelete(false);setReceipt(null);}} className="h-9 rounded-sm border border-line bg-surface-base px-2 text-fg-0">
            <option value="">Choose a setup…</option>{profiles.map(p=><option key={p.id} value={p.id}>{p.name}</option>)}
          </select>
        </label>
        <Button variant="primary" onClick={()=>void apply()} disabled={locked||!selected||edited}>Apply saved setup</Button>
        <Button onClick={()=>setConfirmDelete(true)} disabled={locked||!selected}>Delete setup</Button>
      </div>
      {confirmDelete&&<div className="mt-3 flex flex-wrap items-center gap-2 text-sm text-fg-2">Delete “{selected?.name}”?<Button size="sm" variant="danger" onClick={()=>void remove()} disabled={locked}>Confirm delete</Button><Button size="sm" onClick={()=>setConfirmDelete(false)}>Cancel</Button></div>}
      <p className="mt-3 text-xs text-fg-3">Applying replaces the current Mac remap and macro library and any components included below. Macros do not play automatically. Keep the keyboard connected until the TFT upload finishes.</p>
      {edited&&<p className="mt-2 text-sm text-fg-2">Unsaved setup edits. Save before applying.</p>}
      <div className="mt-4 flex flex-wrap gap-2"><Button size="sm" onClick={()=>void files("import")} disabled={locked}>Import setups…</Button><Button size="sm" onClick={()=>void files("export")} disabled={locked||!selected}>Export selected setup…</Button><Button size="sm" onClick={()=>void files("export",true)} disabled={locked||!profiles.length}>Export all setups…</Button></div>
    </Card>
    <AppSetupRules profiles={profiles} />
    <Card title="Setup contents" action={<Button size="sm" onClick={()=>void capture()} disabled={locked}>Capture app settings</Button>}>
      <p className="mb-4 text-sm text-fg-2">Capture the last settings applied through this app and saved remaps/macros. These values are stored on this Mac; they are not read back from the keyboard. Configure other views and capture again to update this draft.</p>
      {!draft?<p className="text-sm text-fg-2">{working?"Loading…":"Capture app settings to start a setup."}</p>:<div className="space-y-5">
        <label className="grid gap-1 text-sm text-fg-2">Lighting
          <select aria-label="Setup lighting source" disabled={locked} value="" onChange={e=>{if(e.target.value==="captured")edit({lighting:captured?.lighting??null});else if(e.target.value==="none")edit({lighting:null});else{const p=colours.find(p=>p.id===Number(e.target.value));if(p)edit({lighting:{kind:"per_key",profile:p.profile}});}}} className="h-9 rounded-sm border border-line bg-surface-base px-2 text-fg-0">
            <option value="">{draft.lighting?(draft.lighting.kind==="global"?`${draft.lighting.config.mode}, brightness ${draft.lighting.config.brightness}/5`:`Per-key colours, brightness ${draft.lighting.profile.brightness}/5`):"Leave lighting unchanged"}</option>
            <option value="captured">Use captured app lighting</option>{colours.map(p=><option key={p.id} value={p.id}>Colour profile: {p.name}</option>)}<option value="none">Leave lighting unchanged</option>
          </select>
        </label>
        <label className="grid max-w-sm gap-1 text-sm text-fg-2">Sleep timer
          <select aria-label="Setup sleep timer" disabled={locked} value={draft.sleep_minutes??""} onChange={e=>edit({sleep_minutes:e.target.value===""?null:Number(e.target.value)})} className="h-9 rounded-sm border border-line bg-surface-base px-2 text-fg-0">
            <option value="">Leave unchanged / not captured</option><option value="0">Never</option><option value="1">1 minute</option><option value="5">5 minutes</option><option value="30">30 minutes</option>
          </select>
        </label>
        <div className="text-sm text-fg-2"><p className="font-medium text-fg-0">Mac key remaps and macros</p><p>{draft.host.enabled?"Remaps enabled":"Remaps disabled"} · {draft.host.remaps.length} remaps · {draft.host.macros.length} macros · {draft.host.macros.reduce((n,m)=>n+m.actions.length,0)} macro actions</p><p className="mt-1 text-xs">These replace the current Mac configuration. Keep the app open for remaps and macros.</p>
          <details className="mt-3"><summary className="cursor-pointer">Review key remaps and macro actions</summary>
            <div className="mt-2 max-h-72 space-y-2 overflow-auto rounded-sm border border-line p-3">
              {draft.host.remaps.map((r,i)=><p key={i}>{hidName(r.source)} → {r.target.kind==="key"?hidName(r.target.usage):`Macro M${r.target.macro_id+1}`}</p>)}
              {draft.host.macros.map(m=><details key={m.macro_id}><summary className="cursor-pointer">{m.name||`Macro M${m.macro_id+1}`} · {m.actions.length} actions</summary><pre className="mt-2 whitespace-pre-wrap text-xs">{m.actions.map(a=>`${a.delay_ms} ms wait · ${a.is_press?"Press":"Release"} ${hidName(a.key_code)}`).join("\n")}</pre></details>)}
            </div>
          </details>
        </div>
        <div className="space-y-3 text-sm text-fg-2"><p className="font-medium text-fg-0">TFT image / GIF</p>
          {draft.display?<div className="flex flex-wrap items-center gap-4"><canvas ref={preview} width={128} height={128} aria-label="Setup TFT preview" className="h-32 w-32 rounded-md border border-line bg-black" style={{imageRendering:"pixelated"}}/><p>{draft.display.name} · {draft.display.animation.frames.length} frame(s), embedded in this setup</p><Button size="sm" onClick={()=>edit({display:null})} disabled={locked}>Leave display unchanged</Button></div>:<p>No display captured. Choose an image/GIF to include it, or leave the keyboard screen unchanged.</p>}
          <label className="grid gap-2">Choose setup image / GIF<input aria-label="Setup TFT image file" type="file" accept="image/png,image/jpeg,image/webp,image/gif" disabled={locked} onChange={e=>{const file=e.target.files?.[0];if(file)void loadScreen(file);e.target.value="";}}/></label>
        </div>
        <div className="flex flex-wrap items-end gap-2 border-t border-line pt-4"><label className="grid min-w-48 flex-1 gap-1 text-sm text-fg-2">Setup name<input aria-label="Keyboard setup name" value={name} maxLength={64} disabled={locked} onChange={e=>{setName(e.target.value);setEdited(true);}} placeholder="e.g. Work, Gaming, Editing" className="h-9 rounded-sm border border-line bg-surface-base px-2 text-fg-0"/></label><Button onClick={()=>void save(null)} disabled={locked||!validName}>Save as new setup</Button><Button onClick={()=>void save(selectedId)} disabled={locked||!validName||!selected}>Save setup changes</Button></div>
      </div>}
    </Card>
    <Card title="Cycle complete setups">
      <div className="flex flex-wrap items-end gap-2"><label className="grid min-w-48 flex-1 gap-1 text-sm text-fg-2">Shortcut<input ref={input} aria-label="Cycle complete setups shortcut" readOnly disabled={locked} value={recording?"Press a key combination…":shortcut} onKeyDown={e=>{if(!recording||e.key==="Tab")return;e.preventDefault();if(e.key==="Escape"){setRecording(false);return;}if(["Control","Alt","Meta","Shift"].includes(e.key))return;if(!(e.ctrlKey||e.altKey||e.metaKey)){setError("Include Control, Option, or Command.");return;}setShortcut([e.ctrlKey&&"Control",e.altKey&&"Alt",e.shiftKey&&"Shift",e.metaKey&&"Super",e.code].filter(Boolean).join("+"));setRecording(false);setError(null);}} className="h-9 rounded-sm border border-line bg-surface-base px-2 text-fg-0"/></label><Button size="sm" onClick={()=>{setRecording(true);input.current?.focus();}} disabled={locked}>Record setup shortcut</Button><Button size="sm" onClick={()=>void saveShortcut(true)} disabled={locked||recording}>Enable / update setup shortcut</Button><Button size="sm" onClick={()=>void saveShortcut(false)} disabled={locked||!shortcutEnabled}>Disable setup shortcut</Button></div>
      <p className="mt-3 text-xs text-fg-3">{shortcutEnabled?"Enabled":"Disabled"} · Default: Control + Option + Command + S. Applies setups in list order, including remaps, macros and screen uploads, then wraps to the first. Keep the app open; wait for one setup to finish before switching again. Shortcut switching replaces unsaved setup edits. Alt = Option; Super = Command.</p>
    </Card>
  </div>;
}
