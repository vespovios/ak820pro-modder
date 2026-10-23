import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Card, ErrorBanner, KVList } from "../components/ui";
import { PageHeader } from "../components/Layout";
import { SyncCard } from "../components/SyncCard";
import { formatError } from "../errors";
import type { ProbeReport } from "../types";

/** This firmware has setters, but no decoded telemetry/settings getters.
 * Never present cached settings as device read-back. */
export function FeatureSystem() {
  const [verified, setVerified] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [receipt, setReceipt] = useState<string | null>(null);
  const [sleep, setSleepState] = useState<number | null>(null);

  async function verify() {
    setBusy(true); setErr(null);
    try {
      const p = await invoke<ProbeReport>("verify_connection");
      setVerified(p.connected);
    } catch (e) { setVerified(false); setErr(formatError(e)); }
    finally { setBusy(false); }
  }
  useEffect(() => { void verify();void invoke<number|null>("get_saved_sleep").then(setSleepState).catch(()=>{}); }, []);

  async function syncClock() {
    setBusy(true); setErr(null); setReceipt(null);
    try {
      const d = new Date();
      await invoke("sync_keyboard_clock", { time: {
        year: d.getFullYear(), month: d.getMonth() + 1, day: d.getDate(),
        hour: d.getHours(), minute: d.getMinutes(), second: d.getSeconds(),
      } });
      setReceipt(`Clock sent: ${d.toLocaleString()}. Keyboard acknowledged the transaction.`);
    } catch (e) { setErr(formatError(e)); }
    finally { setBusy(false); }
  }

  async function setSleep(minutes: number) {
    setBusy(true); setErr(null); setReceipt(null);
    try {
      await invoke("set_sleep_minutes", { minutes });
      setSleepState(minutes);
      setReceipt("Sleep setting sent. Keyboard acknowledged the transaction.");
    } catch (e) { setErr(formatError(e)); }
    finally { setBusy(false); }
  }

  return <>
    <PageHeader title="System" description="Clock and standby settings for your AK820 revision."
      action={<Button onClick={() => void verify()} disabled={busy}>{busy ? "Working…" : "Check connection"}</Button>} />
    <ErrorBanner>{err}</ErrorBanner>
    {receipt && <p role="status" className="mb-5 rounded-md border border-good/40 bg-good/10 p-3 text-sm text-fg-0">{receipt}</p>}
    <div className="grid gap-6">
      <Card title="Device">
        <KVList rows={[
          { label: "Control channel", value: "USB feature reports · interface 3" },
          { label: "Communication", value: verified ? <Badge tone="good">handshake verified</Badge> : "Not verified" },
          { label: "Available controls", value: "Lighting effects, clock sync and standby" },
        ]} />
        <p className="mt-4 text-sm text-fg-2">Firmware, battery level and current settings are not exposed by this revision's decoded protocol.</p>
      </Card>
      <div className="grid gap-6 lg:grid-cols-2">
        <Card title="Display clock">
          <p className="mb-4 text-sm text-fg-2">Set the keyboard clock to this Mac's local date and time. Select the clock face on the keyboard to see it.</p>
          <Button variant="primary" onClick={() => void syncClock()} disabled={busy}>Sync clock now</Button>
        </Card>
        <Card title="Sleep timer">
          <p className="mb-4 text-sm text-fg-2">Choose how long the keyboard stays lit when idle.</p>
          <div className="flex flex-wrap gap-2">{[0, 1, 5, 30].map(minutes =>
            <Button key={minutes} onClick={() => void setSleep(minutes)} disabled={busy}
              variant={sleep === minutes ? "ghost-active" : "ghost"}>{minutes === 0 ? "Never" : `${minutes} min`}</Button>
          )}</div>
          <p className="mt-4 text-xs text-fg-3">{sleep === null ? "Current value cannot be read from the keyboard." : `Last saved by this app: ${sleep === 0 ? "never" : `${sleep} minutes`}.`}</p>
        </Card>
      </div>
      <SyncCard />
    </div>
  </>;
}
