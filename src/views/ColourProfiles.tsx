import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, ErrorBanner } from "../components/ui";
import { formatError } from "../errors";
import { type ColourProfile, type AppliedColourProfile, useColourEvent } from "../colour-profile-events";
interface NamedProfile {
  id: number;
  name: string;
  profile: ColourProfile;
}

export function ColourProfiles({ draft, disabled, onUse }: {
  draft: ColourProfile;
  disabled: boolean;
  onUse: (profile: ColourProfile, id: number) => Promise<boolean>;
}) {
  const [profiles, setProfiles] = useState<NamedProfile[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [receipt, setReceipt] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [shortcut, setShortcut] = useState("Control+Alt+Super+KeyP");
  const [shortcutEnabled, setShortcutEnabled] = useState(false);
  const [recording, setRecording] = useState(false);
  const shortcutInput = useRef<HTMLInputElement>(null);
  const selected = profiles.find(p => p.id === selectedId);
  const locked = disabled || busy || !loaded;
  const validName = name.trim().length > 0 && [...name.trim()].length <= 64;

  async function reload() {
    setBusy(true);
    setErr(null);
    try {
      setProfiles(await invoke<NamedProfile[]>("list_feature_rgb_profiles"));
      setLoaded(true);
    } catch (e) { setErr(formatError(e)); }
    finally { setBusy(false); }
  }
  useEffect(() => { void reload(); }, []);
  useEffect(() => {
    void invoke<{settings:{enabled:boolean;shortcut:string};error:string|null}>("colour_shortcut_status").then(s => {
      setShortcut(s.settings.shortcut); setShortcutEnabled(s.settings.enabled);
      if(s.error) setErr(s.error);
    }).catch(e => setErr(formatError(e)));
  }, []);
  useColourEvent<AppliedColourProfile>("feature-rgb-applied", applied => {
    if (applied.id !== null) setSelectedId(applied.id);
    if (applied.name) { setName(applied.name); setReceipt(`“${applied.name}” applied to keyboard.`); }
  });

  async function files(action: "import" | "export", all = false) {
    setBusy(true); setErr(null); setReceipt(null);
    try {
      if (action === "import") {
        const added = await invoke<NamedProfile[] | null>("import_colour_profiles");
        if (added?.length) {
          setProfiles(await invoke<NamedProfile[]>("list_feature_rgb_profiles"));
          setSelectedId(added[0].id); setName(added[0].name);
          setReceipt(`Imported ${added.length} profile${added.length === 1 ? "" : "s"}. Choose Use profile to apply.`);
        }
      } else {
        const path = await invoke<string | null>("export_colour_profiles", {id: all ? null : selectedId});
        if (path) setReceipt(`Exported to ${path}`);
      }
    } catch(e) {setErr(formatError(e));}
    finally {setBusy(false);}
  }
  async function saveShortcut(enabled: boolean) {
    setBusy(true); setErr(null); setReceipt(null);
    try {
      await invoke("save_colour_shortcut", {settings:{enabled,shortcut}});
      setShortcutEnabled(enabled); setRecording(false);
      setReceipt(enabled ? "Profile cycling shortcut enabled. Keep the app open." : "Profile cycling shortcut disabled.");
    } catch(e) {setErr(formatError(e));}
    finally {setBusy(false);}
  }

  async function save(id: number | null) {
    setBusy(true); setErr(null); setReceipt(null); setConfirmDelete(false);
    try {
      const saved = await invoke<NamedProfile>("save_feature_rgb_profile", { id, name, profile: draft });
      setProfiles(prev => id === null ? [...prev, saved] : prev.map(p => p.id === id ? saved : p));
      setSelectedId(saved.id); setName(saved.name);
      setReceipt(`Saved “${saved.name}” on this Mac.`);
    } catch (e) { setErr(formatError(e)); }
    finally { setBusy(false); }
  }

  async function useProfile() {
    if (!selected) return;
    setBusy(true); setErr(null); setReceipt(null); setConfirmDelete(false);
    try {
      if (await onUse(selected.profile, selected.id)) setReceipt(`“${selected.name}” applied to keyboard.`);
    } catch (e) { setErr(formatError(e)); }
    finally { setBusy(false); }
  }

  async function remove() {
    if (!selected) return;
    setBusy(true); setErr(null); setReceipt(null);
    try {
      await invoke("delete_feature_rgb_profile", { id: selected.id });
      setProfiles(prev => prev.filter(p => p.id !== selected.id));
      setSelectedId(null); setName(""); setConfirmDelete(false);
      setReceipt(`Deleted “${selected.name}”. The keyboard’s colours are unchanged.`);
    } catch (e) { setErr(formatError(e)); }
    finally { setBusy(false); }
  }

  return <section aria-label="Colour profiles" className="mb-5 rounded-md border border-line bg-surface-base/40 p-3">
    <h3 className="mb-2 text-sm font-medium text-fg-0">Colour profiles</h3>
    <ErrorBanner>{err}</ErrorBanner>
    {!loaded ? <div className="flex items-center gap-3 text-sm text-fg-2">
      {err ? "Could not load saved profiles." : "Loading profiles…"}
      {err && <Button size="sm" onClick={() => void reload()} disabled={busy}>Retry profiles</Button>}
    </div> : <>
      <div className="flex flex-wrap items-end gap-2">
        <label className="grid min-w-48 flex-1 gap-1 text-xs text-fg-2">Saved profile
          <select aria-label="Saved colour profile" value={selectedId ?? ""} disabled={locked}
            onChange={e => {
              const id = e.target.value === "" ? null : Number(e.target.value);
              setSelectedId(id); setName(profiles.find(p => p.id === id)?.name ?? "");
              setConfirmDelete(false); setReceipt(null); setErr(null);
            }} className="h-8 rounded-sm border border-line bg-surface-base px-2 text-sm text-fg-0">
            <option value="">Choose a profile…</option>
            {profiles.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}
          </select>
        </label>
        <Button size="sm" variant="primary" onClick={() => void useProfile()} disabled={locked || !selected}>Use profile</Button>
        <Button size="sm" variant="ghost" onClick={() => setConfirmDelete(true)} disabled={locked || !selected}>Delete profile</Button>
      </div>
      {confirmDelete && <div className="mt-3 flex flex-wrap items-center gap-2 text-sm text-fg-2">
        Delete “{selected?.name}” from saved profiles?
        <Button size="sm" variant="danger" onClick={() => void remove()} disabled={locked}>Confirm delete</Button>
        <Button size="sm" onClick={() => setConfirmDelete(false)} disabled={locked}>Cancel</Button>
      </div>}
      <div className="mt-3 flex flex-wrap items-end gap-2">
        <label className="grid min-w-48 flex-1 gap-1 text-xs text-fg-2">Profile name
          <input aria-label="Colour profile name" value={name} maxLength={64} disabled={locked}
            onChange={e => { setName(e.target.value); setReceipt(null); }} placeholder="e.g. Work or Gaming"
            className="h-8 rounded-sm border border-line bg-surface-base px-2 text-sm text-fg-0" />
        </label>
        <Button size="sm" onClick={() => void save(null)} disabled={locked || !validName}>Save as new</Button>
        <Button size="sm" onClick={() => void save(selectedId)} disabled={locked || !validName || !selected}>Save changes</Button>
      </div>
      <p className="mt-2 text-xs text-fg-3">Save the editor’s colours and brightness on this Mac. Use profile loads and applies the selected colours, replacing any edits in the editor.</p>
      <div className="mt-3 flex flex-wrap gap-2 border-t border-line/60 pt-3">
        <Button size="sm" onClick={() => void files("import")} disabled={locked}>Import profiles…</Button>
        <Button size="sm" onClick={() => void files("export")} disabled={locked || !selected}>Export selected…</Button>
        <Button size="sm" onClick={() => void files("export", true)} disabled={locked || !profiles.length}>Export all…</Button>
      </div>
      <p className="mt-2 text-xs text-fg-3">Share exported JSON files with other ANSI AK820 Pro users. Imports add new profiles; existing names are kept.</p>
      <div className="mt-3 flex flex-wrap items-end gap-2 border-t border-line/60 pt-3">
        <label className="grid min-w-48 flex-1 gap-1 text-xs text-fg-2">Cycle profiles shortcut
          <input ref={shortcutInput} aria-label="Cycle profiles shortcut" readOnly value={recording ? "Press a key combination…" : shortcut}
            disabled={locked} onKeyDown={e => {
              if (!recording || e.key === "Tab") return;
              e.preventDefault();
              if (e.key === "Escape") {setRecording(false);return;}
              if (["Control","Alt","Meta","Shift"].includes(e.key)) return;
              if (!(e.ctrlKey || e.altKey || e.metaKey)) {setErr("Include Control, Option, or Command.");return;}
              const parts = [e.ctrlKey && "Control",e.altKey && "Alt",e.shiftKey && "Shift",e.metaKey && "Super",e.code].filter(Boolean);
              setShortcut(parts.join("+"));setRecording(false);setErr(null);
            }} className="h-8 rounded-sm border border-line bg-surface-base px-2 text-sm text-fg-0" />
        </label>
        <Button size="sm" onClick={() => {setRecording(true);shortcutInput.current?.focus();}} disabled={locked}>Record shortcut</Button>
        <Button size="sm" onClick={() => void saveShortcut(true)} disabled={locked || recording}>Enable / update shortcut</Button>
        <Button size="sm" onClick={() => void saveShortcut(false)} disabled={locked || !shortcutEnabled}>Disable shortcut</Button>
      </div>
      <p className="mt-2 text-xs text-fg-3">{shortcutEnabled ? "Enabled" : "Disabled"} · Cycles through profiles in list order, then returns to the first. Works across apps while AK820 Pro Modder is open. Alt = Option; Super = Command. This also replaces unsaved paint edits.</p>
    </>}
    {receipt && <p role="status" className="mt-2 text-xs text-good">{receipt}</p>}
  </section>;
}
