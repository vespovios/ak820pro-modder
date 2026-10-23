import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { PageHeader } from "../components/Layout";
import { Badge, Button, Card, ErrorBanner, Mono } from "../components/ui";
import { formatError } from "../errors";
import { CODE_TO_HID } from "../data/keyboard-usage";

/* ----------------------------------------------------------------- types -- */

type ActionKind = "keyboard" | "mouse";

interface MacroAction {
  delay_ms: number;
  key_code: number;
  is_press: boolean;
  kind: ActionKind;
}

interface Macro {
  macro_id: number;
  name: string | null;
  actions: MacroAction[];
}

interface MacroLimits {
  slot_count: number;
  byte_limit: number;
  max_actions_per_macro: number;
}

/* --------------------------------------------------- browser → HID maps -- */

/**
 * Subset of `KeyboardEvent.code` → HID Keyboard Usage Page values.
 * Keep this list pragmatic — anything not here gets recorded as keycode 0 with
 * a warning so the user can manually fix the event later.
 */
/** Lookup for the action editor's label column. */
const HID_TO_LABEL: Record<number, string> = Object.fromEntries(
  Object.entries(CODE_TO_HID).map(([code, hid]) => [hid, prettyCode(code)]),
);
function prettyCode(c: string): string {
  if (c.startsWith("Key")) return c.slice(3);
  if (c.startsWith("Digit")) return c.slice(5);
  if (c.startsWith("F") && /^F\d{1,2}$/.test(c)) return c;
  return c.replace(/([A-Z])/g, " $1").trim();
}

/* ----------------------------------------------------------- component -- */

export function Macros({ host = false }: { host?: boolean }) {
  const [remote, setRemote] = useState<Macro[] | null>(null);
  const [draft, setDraft] = useState<Macro[]>([]);
  const [names, setNames] = useState<Record<number, string>>({});
  const [limits, setLimits] = useState<MacroLimits | null>(null);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [recording, setRecording] = useState(false);
  const recordRef = useRef<{ lastAt: number; actions: MacroAction[] } | null>(
    null,
  );

  const [playback, setPlayback] = useState<{ running: boolean; accessibility: boolean; error: string | null }>({ running: false, accessibility: false, error: null });
  const [playReceipt, setPlayReceipt] = useState<string | null>(null);
  const [testText, setTestText] = useState("");
  const wasPlaying = useRef(false);
  async function checkPlayback() {
    try {
      const next = await invoke<typeof playback>("host_playback_status");
      if (wasPlaying.current && !next.running) setPlayReceipt(next.error ? "Playback failed. See the error above." : "Playback finished. Check the destination or test area for its output.");
      wasPlaying.current = next.running;
      setPlayback(next);
    } catch (e) { setErr(formatError(e)); }
  }
  useEffect(() => {
    if (!host) return;
    void checkPlayback();
    const id = window.setInterval(() => void checkPlayback(), 750);
    return () => window.clearInterval(id);
  }, [host]);
  async function play() {
    if (selectedId === null) return;
    setErr(null);
    try {
      await invoke("play_host_macro", { macroId: selectedId });
      setPlayReceipt("Playback starts in 3 seconds. Click the test area below or switch to the destination app.");
      await checkPlayback();
    } catch (e) { setErr(formatError(e)); }
  }
  async function stopPlayback() {
    await invoke("stop_host_macro"); await checkPlayback();
    wasPlaying.current = false;
    setPlayReceipt("Playback stopped. Any held macro keys are released.");
  }

  /* ---------- IO ---------- */

  // Stable callback (no deps that change after first mount). The previous
  // version captured `limits` in the dep array; setting limits after the
  // first call recreated `refresh`, which re-fired the mount effect and
  // queued a SECOND concurrent `get_macros` invoke. Two simultaneous HID
  // invokes both grab the sync `ConnState` mutex from different tokio
  // workers and — together with the 4-second probe_device poll — exhaust
  // the runtime. We hit the exact same deadlock on the System tab in Phase 2.
  const refresh = useCallback(async () => {
    setBusy(true);
    setErr(null);
    try {
      const l = await invoke<MacroLimits>(host ? "host_macro_limits" : "macro_limits");
      setLimits(l);
      const m = await invoke<Macro[]>(host ? "get_host_macros" : "get_macros");
      setRemote(m);
      setDraft(m.map(cloneMacro));
      // Names are host-side only — preserve through reads. Drop entries for
      // slots that no longer exist on the device.
      setNames((prev) => {
        const next: Record<number, string> = {};
        for (const macro of m) {
          if (host) next[macro.macro_id] = macro.name ?? "";
          else if (prev[macro.macro_id]) next[macro.macro_id] = prev[macro.macro_id];
        }
        return next;
      });
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }, [host]);

  // Guard against the React 19 + Strict-Mode double-invocation pattern as
  // well: if the effect is somehow scheduled twice in quick succession,
  // only let the first call talk to the device.
  const didInit = useRef(false);
  useEffect(() => {
    if (didInit.current) return;
    didInit.current = true;
    refresh();
  }, [refresh]);

  /* ---------- recorder ---------- */

  useEffect(() => {
    if (!recording || selectedId === null) return;
    recordRef.current = { lastAt: performance.now(), actions: [] };

    function captureEvent(e: KeyboardEvent, isPress: boolean) {
      // Don't let the browser handle these — they'd e.g. open DevTools.
      e.preventDefault();
      e.stopPropagation();
      if (recordRef.current === null) return;
      if (host && e.code === "Escape") { if (isPress) stopRecording(); return; }
      if (host && (e.repeat || CODE_TO_HID[e.code] === undefined)) return;
      const now = performance.now();
      const delay = Math.min(
        65535,
        Math.max(0, Math.round(now - recordRef.current.lastAt)),
      );
      const hid = CODE_TO_HID[e.code] ?? 0;
      recordRef.current.actions.push({
        delay_ms: delay,
        key_code: hid,
        is_press: isPress,
        kind: "keyboard",
      });
      recordRef.current.lastAt = now;
    }

    const down = (e: KeyboardEvent) => captureEvent(e, true);
    const up = (e: KeyboardEvent) => captureEvent(e, false);
    const blur = () => { if (host) stopRecording(); };
    window.addEventListener("blur", blur);
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    return () => {
      window.removeEventListener("blur", blur);
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("keyup", up, true);
    };
  }, [recording, selectedId, host]);

  function stopRecording() {
    setRecording(false);
    if (recordRef.current === null || selectedId === null) {
      recordRef.current = null;
      return;
    }
    let captured = recordRef.current.actions;
    if (host) {
      const held = new Set<number>();
      captured = captured.filter(action => {
        if (action.is_press) { if (held.has(action.key_code)) return false; held.add(action.key_code); return true; }
        return held.delete(action.key_code);
      });
      if (captured.length) captured[0] = { ...captured[0], delay_ms: 0 };
      captured.push(...Array.from(held).reverse().map(key_code => ({ key_code, delay_ms: 0, is_press: false, kind: "keyboard" as const })));
    }
    recordRef.current = null;
    if (captured.length === 0) return;
    setDraft((d) =>
      d.map((m) =>
        m.macro_id === selectedId
          ? { ...m, actions: [...m.actions, ...captured] }
          : m,
      ),
    );
  }

  /* ---------- editing helpers ---------- */

  const selected = useMemo(
    () => draft.find((m) => m.macro_id === selectedId) ?? null,
    [draft, selectedId],
  );

  function withSelected(
    update: (m: Macro) => Macro,
  ): void {
    if (selectedId === null) return;
    setDraft((d) => d.map((m) => (m.macro_id === selectedId ? update(m) : m)));
  }

  function updateDelay(idx: number, value: number) {
    withSelected((m) => {
      const next = [...m.actions];
      next[idx] = { ...next[idx], delay_ms: Math.max(0, Math.min(65535, value)) };
      return { ...m, actions: next };
    });
  }

  function removeAction(idx: number) {
    withSelected((m) => ({ ...m, actions: m.actions.filter((_, i) => i !== idx) }));
  }

  function clearActions() {
    withSelected((m) => ({ ...m, actions: [] }));
  }

  function addMacroSlot() {
    if (limits === null) return;
    // Pick the lowest unused id in [0, slot_count).
    const used = new Set(draft.map((m) => m.macro_id));
    let candidate = 0;
    while (used.has(candidate) && candidate < limits.slot_count) candidate += 1;
    if (candidate >= limits.slot_count) {
      setErr(`All ${limits.slot_count} macro slots are taken.`);
      return;
    }
    const fresh: Macro = { macro_id: candidate, name: null, actions: [] };
    setDraft((d) => [...d, fresh].sort((a, b) => a.macro_id - b.macro_id));
    setSelectedId(candidate);
  }

  function deleteMacro(id: number) {
    setNames(names => Object.fromEntries(Object.entries(names).filter(([key]) => Number(key) !== id)));
    setDraft((d) => d.filter((m) => m.macro_id !== id));
    if (selectedId === id) setSelectedId(null);
  }

  /* ---------- name editing (host-only) ---------- */

  function setName(id: number, name: string) {
    setNames((n) => ({ ...n, [id]: name }));
    if (host) setDraft(list => list.map(m => m.macro_id === id ? { ...m, name } : m));
  }

  /* ---------- save / discard ---------- */

  const dirty = useMemo(() => {
    if (remote === null) return false;
    if (remote.length !== draft.length) return true;
    return host ? JSON.stringify(remote) !== JSON.stringify(draft) : JSON.stringify(stripNames(remote)) !== JSON.stringify(stripNames(draft));
  }, [remote, draft, host]);

  async function save() {
    if (limits === null) return;
    // Validate per-macro byte limit on the client too — backend will reject
    // but we can surface a friendlier message.
    for (const m of draft) {
      if (m.actions.length === 0) continue;
      const size = 4 + m.actions.length * 4;
      if (size > limits.byte_limit) {
        setErr(
          `Macro #${m.macro_id} has ${m.actions.length} actions (${size} B) — exceeds ${limits.byte_limit} B ${host ? "macro" : "device"} limit.`,
        );
        return;
      }
    }
    setBusy(true);
    setErr(null);
    try {
      // Wire format wants the strict list (id+actions). Names stay host-side.
      const payload = draft.map((m) => ({
        macro_id: m.macro_id,
        name: host ? names[m.macro_id] ?? m.name : null as string | null,
        actions: m.actions,
      }));
      await invoke(host ? "save_host_macros" : "set_macros", { macros: payload });
      await refresh();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  function discard() {
    if (remote === null) return;
    setDraft(remote.map(cloneMacro));
    if (host) setNames(Object.fromEntries(remote.map(m => [m.macro_id, m.name ?? ""])));
  }

  /* --------------------------------------------------------- render -- */

  return (
    <>
      <PageHeader
        title="Macros"
        description={host ? "Record and replay keyboard sequences on this Mac. Up to 100 macros, 512 events and 60 seconds each." : "Record key sequences once, replay them with a single key press. Up to 100 slots, 79 actions per macro."}
        action={
          <div className="flex gap-2">
            {dirty && (
              <Button variant="ghost" size="sm" onClick={discard} disabled={busy}>
                Discard
              </Button>
            )}
            <Button variant="primary" size="sm" onClick={save} disabled={busy || recording || playback.running || !dirty}>
              {busy ? "Saving…" : dirty ? host ? "Save on this Mac" : "Save to device" : "Saved"}
            </Button>
          </div>
        }
      />

      <ErrorBanner>{err ?? (host ? playback.error : null)}</ErrorBanner>
      {host && <Card title="Mac-side playback" className="mb-6">
        <p className="mb-3 text-sm text-fg-2">Macros run while this app is open. Save a macro, then bind an AK820 key in Keymap & Knob. Press the trigger again to stop a running macro. Recording only listens inside this window.</p>
        <div className="flex flex-wrap items-center gap-3">
          <Badge tone={playback.accessibility ? "good" : "warn"}>{playback.accessibility ? "Accessibility enabled" : "Accessibility permission needed"}</Badge>
          {!playback.accessibility && <Button size="sm" onClick={() => void invoke("open_keyboard_accessibility_settings").catch(e => setErr(formatError(e)))}>Open Accessibility settings</Button>}
          <Button size="sm" onClick={() => void checkPlayback()}>Check permission</Button>
          <Button size="sm" variant="primary" onClick={() => void play()} disabled={!playback.accessibility || dirty || selectedId === null || !selected?.actions.length || recording || playback.running}>Play in 3 seconds</Button>
          <Button size="sm" variant="danger" onClick={() => void stopPlayback()} disabled={!playback.running}>Stop playback</Button>
        </div>
        {!playback.accessibility && <p className="mt-3 text-xs text-fg-3">macOS requires your approval before this app can generate keystrokes in other apps. Add AK820 Pro Modder in Privacy & Security → Accessibility, enable it, then check permission again.</p>}
        {playback.accessibility && <p className="mt-3 text-xs text-fg-3">If you just enabled Accessibility and playback produces no text, quit and reopen this app so macOS refreshes its permission.</p>}
        {playReceipt && <p role="status" className="mt-3 text-sm text-fg-1">{playReceipt}</p>}
        <label className="mt-4 grid gap-2 text-sm text-fg-2">Playback test area
          <textarea aria-label="Macro test area" placeholder="Click Play, then click here to test a saved macro" className="min-h-20 w-full" value={testText} onChange={e => setTestText(e.target.value)} />
        </label>
      </Card>}

      <div className="grid gap-6 lg:grid-cols-[280px_1fr]">
        {/* ----- slot list ----- */}
        <Card title="Slots" action={
          <Button size="sm" variant="ghost" onClick={addMacroSlot} disabled={busy || recording || playback.running || limits === null}>
            + New
          </Button>
        }>
          {remote === null ? (
            <p className="text-sm text-fg-2">Reading…</p>
          ) : draft.length === 0 ? (
            <p className="text-sm text-fg-2">No macros yet. Click <b>+ New</b> to create one.</p>
          ) : (
            <ul className="-mx-2 space-y-px">
              {draft.map((m) => {
                const isSel = m.macro_id === selectedId;
                const displayName = names[m.macro_id] || `Macro M${m.macro_id + 1}`;
                return (
                  <li key={m.macro_id}>
                    <button
                      disabled={recording || playback.running}
                      onClick={() => setSelectedId(m.macro_id)}
                      className={[
                        "flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm transition",
                        isSel
                          ? "bg-surface-raised text-fg-0"
                          : "text-fg-1 hover:bg-surface-elevated/60 hover:text-fg-0",
                      ].join(" ")}
                    >
                      <span className="truncate">{displayName}</span>
                      <span className="ml-2 shrink-0 text-2xs text-fg-3">
                        {m.actions.length}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </Card>

        {/* ----- editor ----- */}
        <Card
          title={selected ? `Macro M${selected.macro_id + 1}` : "Editor"}
          action={
            selected && (
              <Button
                size="sm"
                variant="danger"
                onClick={() => deleteMacro(selected.macro_id)}
                disabled={busy || recording || playback.running}
              >
                Delete
              </Button>
            )
          }
        >
          {selected === null ? (
            <p className="text-sm text-fg-2">
              Select a macro on the left, or create a new one to start recording.
            </p>
          ) : (
            <MacroEditor
              macro={selected}
              limits={limits}
              name={names[selected.macro_id] ?? ""}
              onNameChange={(n) => setName(selected.macro_id, n)}
              host={host}
              recording={recording}
              onStartRecording={() => { if (!playback.running) setRecording(true); }}
              onStopRecording={stopRecording}
              onClear={clearActions}
              onUpdateDelay={updateDelay}
              onRemove={removeAction}
            />
          )}
        </Card>
      </div>
    </>
  );
}

/* ------------------------------------------------------ child: editor -- */

function MacroEditor({
  host,
  macro,
  limits,
  name,
  onNameChange,
  recording,
  onStartRecording,
  onStopRecording,
  onClear,
  onUpdateDelay,
  onRemove,
}: {
  host: boolean;
  macro: Macro;
  limits: MacroLimits | null;
  name: string;
  onNameChange: (n: string) => void;
  recording: boolean;
  onStartRecording: () => void;
  onStopRecording: () => void;
  onClear: () => void;
  onUpdateDelay: (idx: number, value: number) => void;
  onRemove: (idx: number) => void;
}) {
  const sizeBytes = macro.actions.length === 0 ? 0 : 4 + macro.actions.length * 4;
  const sizeBudget = limits?.byte_limit ?? 320;
  const sizeRatio = Math.min(1, host ? macro.actions.length / (limits?.max_actions_per_macro ?? 512) : sizeBytes / sizeBudget);

  return (
    <div className="space-y-5">
      {/* name */}
      <div>
        <label className="kicker mb-1 block">Name (local)</label>
        <input
          value={name}
          onChange={(e) => onNameChange(e.target.value)}
          placeholder={`Macro M${macro.macro_id + 1}`}
          className="w-full rounded-md border border-line bg-surface-elevated/40 px-3 py-2 text-sm text-fg-0 outline-none focus:border-accent-500/60"
        />
        <p className="mt-1 text-xs text-fg-3">
          Names are stored on this Mac only — the keyboard firmware doesn't keep them.
        </p>
      </div>

      {/* recorder + meter */}
      <div className="flex flex-wrap items-center gap-3">
        {recording ? (
          <Button variant="danger" size="sm" onClick={onStopRecording}>
            ⏹ Stop recording
          </Button>
        ) : (
          <Button variant="primary" size="sm" onClick={onStartRecording}>
            ⏺ Record
          </Button>
        )}
        <Button variant="ghost" size="sm" onClick={onClear} disabled={macro.actions.length === 0 || recording}>
          Clear actions
        </Button>
        <div className="ml-auto flex items-center gap-3 text-xs text-fg-2">
          <span>
            <Mono>{macro.actions.length}</Mono> actions
          </span>
          <span className="flex items-center gap-2">
            <span className="relative h-1.5 w-24 overflow-hidden rounded-full bg-surface-base">
              <span
                className={[
                  "absolute inset-y-0 left-0 transition-all",
                  sizeRatio > 0.9 ? "bg-warn" : "bg-accent-500",
                ].join(" ")}
                style={{ width: `${sizeRatio * 100}%` }}
              />
            </span>
            <Mono>{host ? `${macro.actions.length}/${limits?.max_actions_per_macro ?? 512} events` : `${sizeBytes}/${sizeBudget} B`}</Mono>
          </span>
        </div>
      </div>

      {recording && (
        <div className="rounded-md border border-accent-500/40 bg-accent-glow px-3 py-2 text-sm text-fg-1">
          Recording — every keypress in this window goes into the macro.
          <br />
          <span className="text-xs text-fg-3">
            Inter-event delays are captured as wall-clock milliseconds. Click <b>Stop</b> when done.{host && " Escape also stops recording; switching windows stops and releases held keys."}
          </span>
        </div>
      )}

      {/* timeline */}
      {macro.actions.length === 0 ? (
        <p className="text-sm text-fg-3">
          Empty macro. Press <b>Record</b> and type to capture events.
        </p>
      ) : (
        <div className="overflow-hidden rounded-md border border-line">
          <table className="w-full text-sm">
            <thead className="bg-surface-elevated/40 text-xs uppercase tracking-wide text-fg-3">
              <tr>
                <th className="w-10 px-3 py-2 text-left">#</th>
                <th className="px-3 py-2 text-left">Action</th>
                <th className="px-3 py-2 text-left">Key</th>
                <th className="px-3 py-2 text-left">Delay (ms)</th>
                <th className="w-10" />
              </tr>
            </thead>
            <tbody>
              {macro.actions.map((a, i) => (
                <tr key={i} className="border-t border-line/60 hover:bg-surface-elevated/30">
                  <td className="px-3 py-1.5 text-fg-3">{i + 1}</td>
                  <td className="px-3 py-1.5">
                    <Badge tone={a.is_press ? "accent" : "neutral"}>
                      {a.is_press ? "press" : "release"}
                    </Badge>
                  </td>
                  <td className="px-3 py-1.5 font-mono text-fg-1">
                    {HID_TO_LABEL[a.key_code] ?? `0x${a.key_code.toString(16).padStart(2, "0")}`}
                    <span className="ml-2 text-xs text-fg-3">[{a.kind}]</span>
                  </td>
                  <td className="px-3 py-1.5">
                    <input
                      type="number"
                      min={0}
                      max={65535}
                      value={a.delay_ms}
                      onChange={(e) =>
                        onUpdateDelay(i, Number.parseInt(e.target.value || "0", 10))
                      }
                      className="w-20 rounded-sm border border-line bg-surface-base px-2 py-0.5 text-right font-mono text-xs text-fg-0 outline-none focus:border-accent-500/60"
                    />
                  </td>
                  <td className="px-3 py-1.5 text-right">
                    <button
                      onClick={() => onRemove(i)}
                      className="text-fg-3 hover:text-bad"
                      aria-label={`Remove action ${i + 1}`}
                    >
                      ×
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

/* ------------------------------------------------------------ helpers -- */

function cloneMacro(m: Macro): Macro {
  return { macro_id: m.macro_id, name: m.name, actions: m.actions.map((a) => ({ ...a })) };
}

function stripNames(list: Macro[]): Macro[] {
  return list.map((m) => ({ macro_id: m.macro_id, name: null, actions: m.actions }));
}
