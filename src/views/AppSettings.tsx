import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Card, ErrorBanner } from "../components/ui";
import { PageHeader } from "../components/Layout";
import { formatError } from "../errors";

type Status = {start_at_login:boolean;keep_running_on_close:boolean;menu_bar_ready:boolean};
export function AppSettings() {
  const [status,setStatus]=useState<Status|null>(null);
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const [receipt,setReceipt]=useState<string|null>(null);
  async function refresh(){setError(null);try{setStatus(await invoke<Status>("app_lifecycle_status"));}catch(e){setError(formatError(e));}}
  useEffect(()=>{void refresh();},[]);
  async function change(kind:"login"|"close",enabled:boolean){
    setBusy(true);setError(null);setReceipt(null);
    try{
      await invoke(kind==="login"?"set_start_at_login":"set_keep_running_on_close",{enabled});
      setStatus(await invoke<Status>("app_lifecycle_status"));
      setReceipt(kind==="login"?(enabled?"Start at login enabled. The app will launch quietly in the menu bar.":"Start at login disabled."):(enabled?"Closing the window will keep the app running in the menu bar.":"Closing the window will quit the app."));
    }catch(e){setError(formatError(e));}finally{setBusy(false);}
  }
  return <div className="space-y-6">
    <PageHeader title="App settings" description="Choose how AK820 Pro Modder starts and runs on this Mac."/>
    <ErrorBanner>{error}</ErrorBanner>
    {receipt&&<p role="status" className="text-sm text-good">{receipt}</p>}
    <Card title="Startup and menu bar">
      <div className="space-y-6 text-sm text-fg-2">
        <label className="flex items-start gap-3"><input aria-label="Start at login" type="checkbox" checked={status?.start_at_login??false} disabled={busy||!status} onChange={e=>void change("login",e.target.checked)} className="mt-1 accent-accent-500"/><span><span className="block font-medium text-fg-0">Start at login</span><span className="mt-1 block">Launch quietly in the menu bar when you log in. App-based setup switching, shortcuts and Mac key controls can run without opening the window.</span></span></label>
        <label className="flex items-start gap-3"><input aria-label="Keep running when the window closes" type="checkbox" checked={status?.keep_running_on_close??true} disabled={busy||!status||!status.menu_bar_ready} onChange={e=>void change("close",e.target.checked)} className="mt-1 accent-accent-500"/><span><span className="block font-medium text-fg-0">Keep running when the window closes</span><span className="mt-1 block">The red close button hides the window. Use AK820 in the menu bar to show it again or quit. You can also reopen it from the Dock or Applications. When disabled, closing the window quits the app.</span></span></label>
        {status&&!status.menu_bar_ready&&<p className="text-bad">The menu bar icon could not start. Restart the app before hiding its window.</p>}
        <div className="flex flex-wrap gap-2"><Button disabled={busy||!status?.menu_bar_ready} onClick={()=>void invoke("hide_to_menu_bar").catch(e=>setError(formatError(e)))}>Hide to menu bar</Button><Button onClick={()=>void refresh()} disabled={busy}>Refresh settings</Button><Button variant="ghost" onClick={()=>void invoke("quit_keyboard_app").catch(e=>setError(formatError(e)))}>Quit app</Button></div>
        <p className="text-xs text-fg-3">Quit app or Command + Q stops all background activity and Mac key controls. Starting at login is optional and applies to this macOS user. If macOS disables the login item, check System Settings → General → Login Items & Extensions.</p>
      </div>
    </Card>
  </div>;
}
