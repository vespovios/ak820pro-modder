import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Card, ErrorBanner, Toggle } from "../components/ui";
import { PageHeader } from "../components/Layout";
import { KeyboardSurface, type Keymap, type KeyAction } from "./Keymap";
import { useLayout } from "../data/layouts/use-layout";
import { hidName } from "../data/hid-usage-names";
import { CODE_TO_HID } from "../data/keyboard-usage";
import { formatError } from "../errors";

export type HostTarget = { kind: "key"; usage: number } | { kind: "macro"; macro_id: number };
export interface HostRemap { source: number; target: HostTarget }
interface SavedMacro { macro_id: number; name: string | null; actions: unknown[] }
export interface HostStatus {
  config: { enabled: boolean; remaps: HostRemap[]; macros: SavedMacro[]; markers: Record<string, number> };
  services: number; accessibility: boolean; input_monitoring: boolean; running: boolean; error: string | null;
}
const OPTIONS = Object.entries(CODE_TO_HID).map(([code, usage]) => ({ code, usage }))
  .sort((a, b) => a.usage - b.usage);
function label(usage: number) {
  if (usage >= 104 && usage <= 115) return `F${usage - 91}`;
  return hidName(usage);
}

export function HostKeymap() {
  const { layout } = useLayout();
  const [status, setStatus] = useState<HostStatus | null>(null);
  const [draft, setDraft] = useState<HostRemap[]>([]);
  const [enabled, setEnabled] = useState(true);
  const [source, setSource] = useState<number | null>(null);
  const [destination, setDestination] = useState("key:41");
  const [capture, setCapture] = useState(false);
  const [lastKey, setLastKey] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [receipt, setReceipt] = useState<string | null>(null);
  const dirty = status !== null && (enabled !== status.config.enabled || JSON.stringify(draft) !== JSON.stringify(status.config.remaps));

  function accept(next: HostStatus) { setStatus(next); setDraft(next.config.remaps); setEnabled(next.config.enabled); }
  async function load() {
    setBusy(true); setError(null);
    try { accept(await invoke<HostStatus>("get_host_controls")); }
    catch (e) { setError(formatError(e)); }
    finally { setBusy(false); }
  }
  useEffect(() => { void load(); }, []);
  useEffect(() => {
    if (!capture) return;
    const onKey = (event: KeyboardEvent) => {
      event.preventDefault(); event.stopPropagation();
      const usage = CODE_TO_HID[event.code];
      if (usage === undefined) { setError(`Cannot remap ${event.code || "this key"}.`); return; }
      setSource(usage); setCapture(false);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [capture]);

  const slots: Keymap = useMemo(() => {
    const slots: KeyAction[] = Array.from({ length: 128 }, () => ({ kind: "default" }));
    for (const key of layout.rows.flat()) {
      const mapping = draft.find(m => m.source === key.hid);
      if (mapping) slots[key.slot] = mapping.target.kind === "key"
        ? { kind: "keyboard", usage: mapping.target.usage }
        : { kind: "macro", macro_id: mapping.target.macro_id, param2: 0, param3: 0 };
    }
    return { slots };
  }, [draft, layout]);
  const selectedSlot = layout.rows.flat().find(key => key.hid === source)?.slot ?? null;
  const changedSlots = new Set(layout.rows.flat().filter(key =>
    JSON.stringify(draft.find(m => m.source === key.hid)) !== JSON.stringify(status?.config.remaps.find(m => m.source === key.hid))
  ).map(key => key.slot));

  function add() {
    if (source === null) return;
    const [kind, number] = destination.split(":");
    const value = Number(number);
    if (kind === "key" && value === source) { setError("Choose a different destination key."); return; }
    const target: HostTarget = kind === "macro" ? { kind: "macro", macro_id: value } : { kind: "key", usage: value };
    setDraft([...draft.filter(m => m.source !== source), { source, target }].sort((a, b) => a.source - b.source));
    setError(null); setReceipt(null);
  }
  async function save() {
    setBusy(true); setError(null); setReceipt(null);
    try {
      const next = await invoke<HostStatus>("save_host_remaps", { remaps: draft, enabled });
      accept(next);
      setReceipt(next.services > 0 ? "Saved on this Mac. AK820 mappings verified by macOS read-back." : "Saved on this Mac. Connect the AK820 by USB to apply.");
    } catch (e) { setError(formatError(e)); }
    finally { setBusy(false); }
  }
  function targetLabel(target: HostTarget) {
    return target.kind === "key" ? label(target.usage)
      : status?.config.macros.find(m => m.macro_id === target.macro_id)?.name || `Macro M${target.macro_id + 1}`;
  }

  return <>
    <PageHeader title="Keymap & Knob" description="Remap your AK820 on this Mac, or bind a key to a recorded macro."
      action={<div className="flex gap-2">
        <Button onClick={() => status && accept(status)} disabled={busy || !dirty}>Discard</Button>
        <Button variant="primary" onClick={() => void save()} disabled={busy || !dirty}>{busy ? "Applying…" : dirty ? "Save & apply" : "Saved"}</Button>
      </div>} />
    <ErrorBanner>{error ?? status?.error}</ErrorBanner>
    {receipt && <p role="status" className="mb-5 rounded-md border border-good/40 bg-good/10 p-3 text-sm text-fg-0">{receipt}</p>}
    <div className="grid gap-6">
      <Card title="Mac-side profile" action={<Toggle checked={enabled} onChange={setEnabled}>{enabled ? "Enabled" : "Paused"}</Toggle>}>
        <p className="text-sm text-fg-2">Mappings apply only to matching wired AK820 keyboards. This app restores them on launch and reconnect, and removes its mappings when you quit. Your other keyboards keep their mappings.</p>
        <p className="mt-2 text-xs text-fg-3">The hardware Fn layer and volume knob remain controlled by the keyboard firmware. Macro triggers need this app open. Pause and save to restore normal keys.</p>
        <div className="mt-3"><Badge tone={status?.services ? "good" : "neutral"}>{status?.services ? `${status.services} AK820 interfaces available` : "Waiting for AK820"}</Badge></div>
        {status && !status.input_monitoring && <div className="mt-4 flex flex-wrap items-center gap-3">
          <p className="text-sm text-fg-2">macOS may require Input Monitoring permission for letter and punctuation remaps. The app records keys only when you start recording inside its window.</p>
          <Button size="sm" onClick={() => void invoke("open_keyboard_input_settings").catch(e => setError(formatError(e)))}>Open Input Monitoring settings</Button>
          <Button size="sm" onClick={() => void load()} disabled={dirty}>Check permission</Button>
        </div>}
      </Card>
      <Card title={`Keyboard · ${layout.displayName}`}>
        <KeyboardSurface layout={layout.rows} km={slots} dirty={changedSlots} selectedSlot={selectedSlot} automations={[]}
          onSelect={slot => {
            const key = layout.rows.flat().find(key => key.slot === slot);
            if (key?.hid === 175) { setError("The hardware Fn key is handled inside the keyboard and cannot be remapped by macOS."); return; }
            setSource(key?.hid ?? null);
          }} />
      </Card>
      <Card title="Add or change a mapping">
        <div className="flex flex-wrap items-end gap-4">
          <label className="grid gap-2 text-sm text-fg-2">AK820 key
            <select aria-label="Source key" value={source ?? ""} onChange={e => setSource(Number(e.target.value))}>
              <option value="" disabled>Choose a key</option>
              {OPTIONS.map(key => <option key={key.code} value={key.usage}>{label(key.usage)} · {key.code}</option>)}
            </select>
          </label>
          <Button onClick={() => setCapture(!capture)}>{capture ? "Listening… click to cancel" : "Capture a key"}</Button>
          <label className="grid gap-2 text-sm text-fg-2">Send instead
            <select aria-label="Destination action" value={destination} onChange={e => setDestination(e.target.value)}>
              <optgroup label="Keyboard key">{OPTIONS.filter(key => key.usage < 104 || key.usage > 115).map(key =>
                <option key={key.code} value={`key:${key.usage}`}>{label(key.usage)} · {key.code}</option>
              )}</optgroup>
              <optgroup label="Saved macros">{status?.config.macros.map(m =>
                <option key={m.macro_id} value={`macro:${m.macro_id}`} disabled={m.actions.length === 0}>{m.name || `Macro M${m.macro_id + 1}`}{m.actions.length ? "" : " (empty)"}</option>
              )}</optgroup>
            </select>
          </label>
          <Button onClick={add} disabled={source === null || busy}>Add to profile</Button>
        </div>
        <p className="mt-3 text-xs text-fg-3">If a physical legend differs from the selected layout, capture the key to use the event macOS receives. Save changes to apply them.</p>
      </Card>
      <Card title="Mappings" action={<Button size="sm" onClick={() => setDraft([])} disabled={!draft.length || busy}>Remove all from profile</Button>}>
        {draft.length === 0 ? <p className="text-sm text-fg-2">No remaps in this profile.</p> : <ul className="divide-y divide-line">{draft.map(mapping =>
          <li key={mapping.source} className="flex items-center justify-between gap-3 py-3 text-sm">
            <span>{label(mapping.source)} → {targetLabel(mapping.target)}</span>
            <Button size="sm" onClick={() => setDraft(draft.filter(m => m.source !== mapping.source))}>Remove</Button>
          </li>
        )}</ul>}
      </Card>
      <Card title="Key test">
        <p className="mb-3 text-sm text-fg-2">Click below, then press the remapped key on your AK820. The event label should show its destination.</p>
        <textarea aria-label="Key test area" placeholder="Test your keys here" className="min-h-20 w-full" onKeyDown={e => setLastKey(`${e.code} · ${e.key}`)} />
        {lastKey && <p className="mt-2 text-sm text-fg-1">Last event: {lastKey}</p>}
      </Card>
    </div>
  </>;
}
