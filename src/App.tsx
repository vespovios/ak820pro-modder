import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Automations } from "./views/Automations";
import { AppSettings } from "./views/AppSettings";
import { Connect } from "./views/Connect";
import { Keymap } from "./views/Keymap";
import { HostKeymap } from "./views/HostKeymap";
import { Lighting } from "./views/Lighting";
import { Macros } from "./views/Macros";
import { Presets } from "./views/Presets";
import { System } from "./views/System";
import { FeatureSystem } from "./views/FeatureSystem";
import { TftDisplay } from "./views/TftDisplay";
import { ICLOUD_SYNC_ENABLED_KEY } from "./components/SyncCard";
import { type AppliedColourProfile, useColourEvent } from "./colour-profile-events";
import { SetupProfiles, type SetupReport } from "./views/SetupProfiles";
import type { LightingConfig } from "./types";
import {
  Layout,
  Plug,
  Bulb,
  Settings,
  Keyboard,
  Macro,
  Screen,
  Automation,
  Preset,
  type NavItem,
} from "./components/Layout";

type Tab =
  | "app-settings"
  | "connect"
  | "lighting"
  | "setups"
  | "system"
  | "keymap"
  | "macros"
  | "automations"
  | "presets"
  | "tft";

const ICON_PROPS = { size: 16, strokeWidth: 1.6 } as const;

const NAV: NavItem<Tab>[] = [
  { id: "connect", label: "Connectivity", icon: <Plug {...ICON_PROPS} /> },
  { id: "lighting", label: "Lighting", icon: <Bulb {...ICON_PROPS} /> },
  { id: "setups", label: "Setup profiles", icon: <Preset {...ICON_PROPS} /> },
  { id: "system", label: "System", icon: <Settings {...ICON_PROPS} /> },
  { id: "keymap", label: "Keymap & Knob", icon: <Keyboard {...ICON_PROPS} /> },
  { id: "macros", label: "Macros", icon: <Macro {...ICON_PROPS} /> },
  { id: "automations", label: "Automations", icon: <Automation {...ICON_PROPS} /> },
  { id: "presets", label: "Presets", icon: <Preset {...ICON_PROPS} /> },
  { id: "app-settings", label: "App settings", icon: <Settings {...ICON_PROPS} /> },
  { id: "tft", label: "TFT Display", icon: <Screen {...ICON_PROPS} />, comingSoon: true },
];

interface ProbeReport {
  connected: boolean;
  interface: number;
  product: string | null;
  legacy: boolean;
}

export default function App() {
  const [tab, setTab] = useState<Tab>("connect");
  useColourEvent<null>("open-app-settings",()=>setTab("app-settings"));
  useColourEvent<null>("open-setup-profiles",()=>setTab("setups"));
  const [probe, setProbe] = useState<ProbeReport | null>(null);
  const legacy = probe?.legacy ?? false;
  const [colourNotice,setColourNotice]=useState<string|null>(null);
  const [setupRevision,setSetupRevision]=useState(0);
  useColourEvent<LightingConfig>("setup-global-lighting", config=>{
    try{window.localStorage.setItem("ak820:lighting:feature",JSON.stringify(config));}catch{/* optional UI preference */}
  });
  useColourEvent<{name:string;component:string}>("setup-progress",p=>setColourNotice(`${p.name}: applying ${p.component}…`));
  useColourEvent<string>("setup-switch-error",error=>setColourNotice(`Setup switch failed: ${error}`));
  useColourEvent<SetupReport>("setup-applied",report=>{
    setColourNotice(report.success?`Setup applied: ${report.name}${report.warnings.length?` · ${report.warnings.join(" ")}`:""}`:`Setup stopped at ${report.failed}: ${report.error}. Completed: ${report.completed.join(", ")||"none"}.`);
    setSetupRevision(n=>n+1);
  });
  useColourEvent<AppliedColourProfile>("feature-rgb-applied", applied => {
    setColourNotice(applied.name ? `Colour profile: ${applied.name}` : "Custom colours applied");
    try {
      const old=JSON.parse(window.localStorage.getItem("ak820:lighting:feature")??"null");
      window.localStorage.setItem("ak820:lighting:feature",JSON.stringify({...old,mode:"custom",color:old?.color??"7C5CFF",brightness:applied.profile.brightness,speed:old?.speed??3,direction:old?.direction??"left",color_mode:0,effect_mode_type:0,secondary:null}));
    } catch { /* optional view preference */ }
  });
  useColourEvent<string>("feature-rgb-error", error => setColourNotice(`Colour profile switch failed: ${error}`));

  // iCloud auto-pull on app start. Fire-and-forget — if the iCloud
  // copy of automations.json is newer than the local one, the backend
  // overwrites the local file before any view requests its contents.
  // The frontend never sees the in-flight state because all
  // automation-consuming views fetch on mount, and this effect runs
  // before any view-specific effect.
  useEffect(() => {
    let enabled = false;
    try {
      enabled = window.localStorage.getItem(ICLOUD_SYNC_ENABLED_KEY) === "true";
    } catch {
      /* private-browsing — ignore */
    }
    if (!enabled) return;
    void invoke("icloud_sync_pull").catch(() => {
      // Pull failure on startup shouldn't bubble — user sees the
      // error inline if they open the System → Sync card.
    });
  }, []);

  useEffect(() => {
    let alive = true;

    // Defer first probe by a frame so React finishes mounting cleanly before
    // the first IPC call. Without this we have seen WKWebView freezes that
    // we can't reproduce in Chromium — likely a startup-time ordering issue.
    const start = window.setTimeout(async function tick() {
      if (!alive) return;
      try {
        const p = await invoke<ProbeReport>("probe_device");
        if (alive) setProbe(p);
      } catch {
        if (alive) setProbe(null);
      }
      if (alive) window.setTimeout(tick, 4000);
    }, 200);

    return () => {
      alive = false;
      window.clearTimeout(start);
    };
  }, []);

  const reconnect = async () => {
    try { await invoke("force_reconnect"); } catch { /* ignored */ }
  };

  return (
    <Layout
      nav={NAV.map(item => item.id === "tft" ? { ...item, comingSoon: !legacy } : item)}
      active={tab}
      onSelect={setTab}
      connection={probe ? { connected: probe.connected, product: probe.product } : undefined}
      onReconnect={reconnect}
      wide={tab === "keymap"}
    >
      {colourNotice && <div role="status" className="mb-4 flex items-center justify-between gap-3 rounded-md border border-line bg-surface-elevated p-3 text-sm text-fg-1"><span>{colourNotice}</span><button onClick={()=>setColourNotice(null)} aria-label="Dismiss colour profile status">Dismiss</button></div>}
      {tab === "app-settings" && <AppSettings />}
      {tab === "connect" && <Connect onReconnect={reconnect} />}
      {tab === "lighting" && <Lighting key={setupRevision} legacy={legacy} />}
      {tab === "setups" && <SetupProfiles legacy={legacy} />}
      {tab === "system" && (legacy ? <FeatureSystem key={setupRevision} /> : <System />)}
      {tab === "keymap" && (legacy ? <HostKeymap key={setupRevision} /> : <Keymap />)}
      {tab === "macros" && <Macros key={`${legacy?"host":"device"}:${setupRevision}`} host={legacy} />}
      {tab === "automations" && <Automations />}
      {tab === "presets" && <Presets legacy={legacy} />}
      {tab === "tft" && legacy && <TftDisplay key={setupRevision} />}
    </Layout>
  );
}
