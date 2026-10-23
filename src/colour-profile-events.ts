import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import type { CustomLedMap } from "./types";

export interface ColourProfile { map: CustomLedMap; brightness: number }
export interface AppliedColourProfile { id: number | null; name: string | null; profile: ColourProfile }

export function useColourEvent<T>(event: string, handler: (payload: T) => void) {
  const latest = useRef(handler);
  latest.current = handler;
  useEffect(() => {
    let alive = true;
    let stop: (() => void) | undefined;
    void listen<T>(event, e => { if (alive) latest.current(e.payload); }).then(unlisten => {
      if (alive) stop = unlisten; else unlisten();
    }).catch(() => {});
    return () => { alive = false; stop?.(); };
  }, [event]);
}
