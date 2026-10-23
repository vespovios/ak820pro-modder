/**
 * Per-key RGB paint surface.
 *
 * Reached from the Lighting view when the user picks the "custom" effect
 * mode (0x80). Lets the user click any physical key on the AK820 Pro
 * ISO-DE layout, pick a colour, and live-write the result via
 * `SET_CUSTOM_LED_DATA` (cmd 36) followed by `SET_LED_EFFECT` mode 0x80
 * so the firmware renders from the custom buffer.
 *
 * State is owned locally — we deliberately don't share it with the
 * Lighting view's `LightingConfig` because the wire path is different
 * (per-key buffer vs effect parameters). On save, we issue:
 *   1. `set_custom_led(map)` — full 128 × 4 B buffer
 *   2. `apply_lighting({ mode: "custom", … })` — switch the firmware
 *      to read from the buffer we just wrote
 *
 * Both writes are sequential (HID mutex; see `tokio::sync::Mutex` in
 * `src-tauri/src/lib.rs`). On mount we read the existing buffer so the
 * user picks up where they left off if they cycled away to another mode.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Card, ErrorBanner, Mono, Slider } from "../components/ui";
import { type PhysicalKey } from "../data/layouts";
import { useLayout } from "../data/layouts/use-layout";
import type { CustomLedMap, LedColor, LightingConfig } from "../types";
import { formatError } from "../errors";
import { ColourProfiles } from "./ColourProfiles";
import { type ColourProfile, type AppliedColourProfile, useColourEvent } from "../colour-profile-events";

interface Props {
  /** Brightness + speed from the parent Lighting view — unused by the
   *  firmware in custom mode but we still propagate so the apply_lighting
   *  payload roundtrips other fields cleanly. */
  inheritedConfig: LightingConfig;
  legacy?: boolean;
}

const LED_COUNT = 128;
const DEFAULT_PAINT = "FFFFFF";
const APPLY_DEBOUNCE_MS = 120;

export function CustomLightingPaint({ inheritedConfig, legacy = false }: Props) {
  const { layout, layoutId } = useLayout();
  const layoutRows = legacy ? layout.rows.map(row => row.map(k => k.slot === 107 ? { ...k, label: "Home" } : k)) : layout.rows;
  const [remote, setRemote] = useState<CustomLedMap | null>(null);
  const [draft, setDraft] = useState<LedColor[]>(() => emptyMap());
  const [paintColor, setPaintColor] = useState<string>(DEFAULT_PAINT);
  const [selected, setSelected] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [lastApplied, setLastApplied] = useState<string | null>(null);
  const [autoApply, setAutoApply] = useState(!legacy);
  const [brightness, setBrightness] = useState(inheritedConfig.brightness);
  const [savedBrightness, setSavedBrightness] = useState(inheritedConfig.brightness);
  const alive = useRef(true);

  const inflight = useRef(false);
  const queued = useRef<LedColor[] | null>(null);
  const pending = useRef<number | null>(null);
  const initRef = useRef(false);
  useColourEvent<AppliedColourProfile>("feature-rgb-applied", applied => {
    if (!legacy) return;
    setDraft(applied.profile.map.leds.map(l => ({...l})));
    setRemote(applied.profile.map);
    setBrightness(applied.profile.brightness); setSavedBrightness(applied.profile.brightness);
    setLastApplied(new Date().toLocaleTimeString()); setErr(null);
  });

  /* ----- IO ---------------------------------------------------------- */

  const refresh = useCallback(async () => {
    setBusy(true);
    setErr(null);
    try {
      let m: CustomLedMap;
      if (legacy) {
        const saved = await invoke<{ map: CustomLedMap; brightness: number } | null>("get_feature_rgb_profile");
        if (saved) {
          m = saved.map; setBrightness(saved.brightness); setSavedBrightness(saved.brightness);
        } else {
          const { r, g, b } = parseHex(inheritedConfig.color);
          m = { leds: emptyMap().map(l => ({ ...l, red: r, green: g, blue: b })) };
        }
      } else m = await invoke<CustomLedMap>("get_custom_led");
      setRemote(m);
      setDraft(m.leds.map((c) => ({ ...c })));
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }, [legacy, inheritedConfig.color]);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      if (pending.current !== null) window.clearTimeout(pending.current);
      queued.current = null;
    };
  }, []);

  useEffect(() => {
    if (initRef.current) return;
    initRef.current = true;
    refresh();
  }, [refresh]);

  const apply = useCallback(
    async (map: LedColor[], level = brightness, profileId: number | null = null): Promise<boolean> => {
      if (inflight.current) {
        queued.current = map;
        return false;
      }
      inflight.current = true;
      setBusy(true);
      setErr(null);
      try {
        if (legacy) {
          await invoke("apply_feature_rgb", { profile: { map: { leds: map }, brightness: level }, profileId });
          try { window.localStorage.setItem("ak820:lighting:feature", JSON.stringify({ ...inheritedConfig, mode: "custom", brightness: level })); } catch { /* optional local preference */ }
          setSavedBrightness(level);
        } else {
        // 1. write the per-key buffer
        await invoke("set_custom_led", { map: { leds: map } });
        // 2. flip the firmware to render from it
        await invoke("apply_lighting", {
          config: { ...inheritedConfig, mode: "custom" } satisfies LightingConfig,
        });
        }
        setRemote({ leds: map.map(l => ({ ...l })) });
        setLastApplied(new Date().toLocaleTimeString());
        return true;
      } catch (e) {
        setErr(formatError(e));
        return false;
      } finally {
        setBusy(false);
        inflight.current = false;
        const drain = queued.current;
        queued.current = null;
        if (drain && alive.current) void apply(drain);
      }
    },
    [inheritedConfig, legacy, brightness],
  );

  async function useProfile(profile: ColourProfile, id: number) {
    const colours = profile.map.leds.map(l => ({ ...l }));
    setDraft(colours);
    setBrightness(profile.brightness);
    setLastApplied(null);
    return apply(colours, profile.brightness, id);
  }

  function scheduleApply(map: LedColor[]) {
    if (pending.current !== null) window.clearTimeout(pending.current);
    pending.current = window.setTimeout(() => {
      pending.current = null;
      void apply(map);
    }, APPLY_DEBOUNCE_MS);
  }

  /* ----- edit ops ---------------------------------------------------- */

  function paintSlot(slot: number, hexColor: string) {
    if (!/^[0-9a-f]{6}$/i.test(hexColor)) return;
    setLastApplied(null);
    const { r, g, b } = parseHex(hexColor);
    setDraft((prev) => {
      const next = prev.slice();
      next[slot] = { led_id: slot, red: r, green: g, blue: b };
      if (autoApply) scheduleApply(next);
      return next;
    });
  }

  function fillAll(hexColor: string) {
    if (!/^[0-9a-f]{6}$/i.test(hexColor)) return;
    setLastApplied(null);
    const { r, g, b } = parseHex(hexColor);
    const next = Array.from({ length: LED_COUNT }, (_, i) => ({
      led_id: i,
      red: r,
      green: g,
      blue: b,
    }));
    setDraft(next);
    if (autoApply) scheduleApply(next);
  }

  function clearAll() {
    setLastApplied(null);
    const next = emptyMap();
    setDraft(next);
    if (autoApply) scheduleApply(next);
  }

  /* ----- diff --------------------------------------------------------- */

  const dirty = useMemo(() => {
    if (!remote) return false;
    if (remote.leds.length !== draft.length || (legacy && brightness !== savedBrightness)) return true;
    for (let i = 0; i < draft.length; i++) {
      const a = remote.leds[i];
      const b = draft[i];
      if (a.red !== b.red || a.green !== b.green || a.blue !== b.blue) {
        return true;
      }
    }
    return false;
  }, [remote, draft, legacy, brightness, savedBrightness]);

  /* --------------------------------------------------------- render -- */

  if (legacy && layoutId !== "ansi") return <Card title="Per-key RGB"><p className="text-sm text-fg-2">This keyboard’s custom colour map is verified for ANSI / US. Select ANSI in the sidebar to use the paint controls.</p></Card>;

  if (remote === null) {
    return (
      <Card title="Per-key RGB">
        <ErrorBanner>{err}</ErrorBanner>
        <p className="text-sm text-fg-2">{err ? "Could not load the colour profile." : legacy ? "Loading saved colours…" : "Reading current LED state…"}</p>
        {err && <Button onClick={() => void refresh()}>Retry</Button>}
      </Card>
    );
  }

  return (
    <Card
      kicker="Per-key paint"
      title="Click any key to paint it"
      action={
        <div className="flex items-center gap-2">
          {lastApplied && (
            <span className="text-xs text-fg-3">applied {lastApplied}</span>
          )}
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void refresh()}
            disabled={busy}
          >
            {legacy ? "Reload saved" : "Reload"}
          </Button>
          <Button
            size="sm"
            variant={autoApply || !dirty ? "ghost" : "primary"}
            onClick={() => void apply(draft)}
            disabled={busy || (!legacy && !dirty)}
          >
            {busy ? "Writing…" : legacy ? "Apply to keyboard" : dirty ? "Apply" : "Saved"}
          </Button>
        </div>
      }
    >
      <ErrorBanner>{err}</ErrorBanner>
      {legacy && <p className="mb-4 text-sm text-fg-2">Choose a brush colour, click keys, then Apply to keyboard. Colours below are your saved or edited profile; they are not read back from the keyboard.</p>}

      {legacy && <ColourProfiles draft={{ map: { leds: draft }, brightness }} disabled={busy} onUse={useProfile} />}

      {/* paint controls */}
      <div className="mb-4 flex flex-wrap items-center gap-3 rounded-md border border-line/60 bg-surface-elevated/40 px-3 py-2">
        <span className="kicker">Brush</span>
        <input
          type="color"
          aria-label="Brush colour picker"
          value={"#" + paintColor}
          onChange={(e) => setPaintColor(e.target.value.replace("#", "").toUpperCase())}
          className="h-7 w-10 cursor-pointer"
        />
        <input
          type="text"
          aria-label="Brush hex colour"
          value={paintColor}
          onChange={(e) =>
            setPaintColor(
              e.target.value.replace(/[^0-9a-fA-F]/g, "").toUpperCase().slice(0, 6),
            )
          }
          className="w-20 rounded-sm border border-line bg-surface-base px-2 py-0.5 font-mono text-xs uppercase text-fg-0 outline-none focus:border-accent-500/60"
        />
        <span className="text-xs text-fg-3">·</span>
        <Button size="sm" variant="ghost" onClick={() => fillAll(paintColor)} disabled={busy || !/^[0-9a-f]{6}$/i.test(paintColor)}>
          Fill all
        </Button>
        <Button size="sm" variant="ghost" onClick={() => clearAll()} disabled={busy}>
          Clear all
        </Button>
        {!legacy && <label className="ml-auto flex cursor-pointer items-center gap-2 text-xs text-fg-2">
          <input
            type="checkbox"
            checked={autoApply}
            onChange={(e) => setAutoApply(e.target.checked)}
            className="h-3.5 w-3.5 accent-accent-500"
          />
          Auto-apply
        </label>}
      </div>

      <PaintSurface
        layout={layoutRows}
        leds={draft}
        selected={selected}
        onPaint={(slot) => {
          if (busy) return;
          setSelected(slot);
          paintSlot(slot, paintColor);
        }}
      />

      {legacy && <div className="mt-5 max-w-sm"><Slider label="Custom lighting brightness" value={brightness} max={5} onChange={v => { if (!busy) { setBrightness(v); setLastApplied(null); } }} /></div>}
      {legacy && lastApplied && <p role="status" className="mt-4 text-sm text-good">Colours applied and saved on this Mac. Keyboard commit acknowledged.</p>}

      <p className="mt-4 border-t border-line/60 pt-3 text-xs text-fg-3">
        {legacy ? <>Switching to a global effect hides your custom colours. Choose custom and Apply to keyboard to restore this profile. {dirty ? "Unapplied changes." : ""}</> : <>Per-key RGB is the lighting mode at byte <Mono>0x80</Mono>. The 20
        animated modes ignore this buffer; switching the lighting mode away
        from <Badge tone="accent">custom</Badge> will hide your paint until
        you switch back. Selected slot:{" "}
        <Mono>{selected === null ? "—" : `#${selected}`}</Mono>.</>}
      </p>
    </Card>
  );
}

/* ------------------------------------------------------ helpers ---- */

function emptyMap(): LedColor[] {
  return Array.from({ length: LED_COUNT }, (_, i) => ({
    led_id: i,
    red: 0,
    green: 0,
    blue: 0,
  }));
}

function parseHex(hex: string): { r: number; g: number; b: number } {
  const clean = hex.replace(/[^0-9a-fA-F]/g, "").padEnd(6, "0").slice(0, 6);
  return {
    r: parseInt(clean.slice(0, 2), 16),
    g: parseInt(clean.slice(2, 4), 16),
    b: parseInt(clean.slice(4, 6), 16),
  };
}

function ledToCss(c: LedColor): string {
  // black caps still need to read as "off" not as solid black with the
  // same tone as the keyboard's base finish — pull them just slightly
  // toward grey so they stay visible
  const off = c.red === 0 && c.green === 0 && c.blue === 0;
  if (off) return "rgb(20,22,26)";
  return `rgb(${c.red},${c.green},${c.blue})`;
}

/* -------------------------------------- paint surface (lightweight) -- */

/** Compact, colour-only render of the keyboard. Click anywhere to paint. */
function PaintSurface({
  layout,
  leds,
  selected,
  onPaint,
}: {
  layout: PhysicalKey[][];
  leds: LedColor[];
  selected: number | null;
  onPaint: (slot: number) => void;
}) {
  const NAV_LABELS = new Set(["Ende", "Bild↑", "Bild↓", "End", "Home", "PgUp", "PgDn"]);
  const mainRows = layout.map((row) =>
    NAV_LABELS.has(row[row.length - 1]?.label ?? "") ? row.slice(0, -1) : row,
  );
  const navByRow = layout.map((row) => {
    const last = row[row.length - 1];
    return last && NAV_LABELS.has(last.label) ? last : null;
  });

  return (
    <div className="flex items-start gap-1.5 overflow-x-auto p-0.5">
      <div className="flex shrink-0 flex-col gap-1">
        {mainRows.map((row, ri) => (
          <div key={ri} className="flex gap-1">
            {row.map((k) => (
              <PaintCap
                key={k.slot}
                k={k}
                led={leds[k.slot]}
                selected={selected === k.slot}
                onClick={() => onPaint(k.slot)}
              />
            ))}
          </div>
        ))}
      </div>
      <div className="flex shrink-0 flex-col gap-1">
        <div style={{ height: 32 }} />
        {navByRow[1] && (
          <PaintCap
            k={navByRow[1]}
            led={leds[navByRow[1].slot]}
            selected={selected === navByRow[1].slot}
            onClick={() => onPaint(navByRow[1]!.slot)}
          />
        )}
        {navByRow[2] && (
          <PaintCap
            k={navByRow[2]}
            led={leds[navByRow[2].slot]}
            selected={selected === navByRow[2].slot}
            onClick={() => onPaint(navByRow[2]!.slot)}
          />
        )}
        {navByRow[3] && (
          <PaintCap
            k={navByRow[3]}
            led={leds[navByRow[3].slot]}
            selected={selected === navByRow[3].slot}
            onClick={() => onPaint(navByRow[3]!.slot)}
          />
        )}
      </div>
    </div>
  );
}

function PaintCap({
  k,
  led,
  selected,
  onClick,
}: {
  k: PhysicalKey;
  led: LedColor | undefined;
  selected: boolean;
  onClick: () => void;
}) {
  const safeLed = led ?? { led_id: k.slot, red: 0, green: 0, blue: 0 };
  const bg = ledToCss(safeLed);
  const bright = safeLed.red + safeLed.green + safeLed.blue;
  const textColor = bright > 380 ? "#0c0e12" : "#e8eaf0";
  const widthClass = k.cls?.includes("w-27")
    ? "w-[60px]"
    : k.cls?.includes("w-24")
      ? "w-[52px]"
      : k.cls?.includes("w-18")
        ? "w-[44px]"
        : k.cls?.includes("flex-1") && k.label === "Spacebar"
          ? "w-[182px]"
          : k.cls?.includes("flex-1")
            ? "w-[68px]"
            : "w-8";

  return (
    <button
      type="button"
      aria-label={`Paint ${k.label}`}
      onClick={onClick}
      className={[
        "relative flex h-8 items-center justify-center rounded-[5px] border text-[10px] font-medium leading-none transition",
        widthClass,
        selected
          ? "border-accent-500 ring-2 ring-accent-500/60 ring-offset-1 ring-offset-surface-base"
          : "border-line/70 hover:border-line-strong",
      ].join(" ")}
      style={{ backgroundColor: bg, color: textColor }}
      title={`Slot ${k.slot} · ${k.label} · rgb(${safeLed.red},${safeLed.green},${safeLed.blue})`}
    >
      <span className="pointer-events-none truncate px-1">{shortLabel(k.label)}</span>
    </button>
  );
}

function shortLabel(label: string): string {
  if (label.length <= 3) return label;
  if (/^F\d{1,2}$/.test(label)) return label;
  // For long labels (Spacebar, L-Shift, …) show the first 1-3 letters.
  if (label.includes("-")) return label.split("-")[1].slice(0, 3);
  return label.slice(0, 3);
}
