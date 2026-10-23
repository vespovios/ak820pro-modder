import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Button, Card, ErrorBanner } from "../components/ui";
import { PageHeader } from "../components/Layout";

export interface Frame { pixels: number[]; delay_ms: number }
interface Prepared { animation: { frames: Frame[] }; timing_adjusted: boolean }
interface Progress { upload_id: string; completed: number; total: number }

export function renderFrame(canvas: HTMLCanvasElement, frame: Frame) {
  const context = canvas.getContext("2d");
  if (!context) return;
  const image = context.createImageData(128, 128);
  for (let i = 0; i < 128 * 128; i++) {
    const pixel = frame.pixels[i * 2] | (frame.pixels[i * 2 + 1] << 8);
    image.data[i * 4] = Math.round(((pixel >> 11) & 31) * 255 / 31);
    image.data[i * 4 + 1] = Math.round(((pixel >> 5) & 63) * 255 / 63);
    image.data[i * 4 + 2] = Math.round((pixel & 31) * 255 / 31);
    image.data[i * 4 + 3] = 255;
  }
  context.putImageData(image, 0, 0);
}

function testImage(): Prepared {
  const pixels: number[] = [];
  for (let y = 0; y < 128; y++) for (let x = 0; x < 128; x++) {
    const pixel = y < 64 ? (x < 64 ? 0xf800 : 0x07e0) : (x < 64 ? 0x001f : 0xffff);
    pixels.push(pixel & 255, pixel >> 8);
  }
  return { animation: { frames: [{ pixels, delay_ms: 0 }] }, timing_adjusted: false };
}

export function TftDisplay() {
  const [prepared, setPrepared] = useState<Prepared | null>(null);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [receipt, setReceipt] = useState<string | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const uploadId = useRef<string | null>(null);
  const alive = useRef(true);
  const userSelected = useRef(false);

  useEffect(()=>{
    let cancelled=false;
    void invoke<{name:string;animation:{frames:Frame[]}}|null>("get_saved_tft").then(saved=>{
      if(saved&&!cancelled&&!userSelected.current){setPrepared({animation:saved.animation,timing_adjusted:false});setName(saved.name);}
    }).catch(()=>{});
    return()=>{cancelled=true;};
  },[]);

  useEffect(() => {
    alive.current = true;
    return () => { alive.current = false; };
  }, []);

  useEffect(() => {
    const subscription = listen<Progress>("tft-progress", event => {
      if (event.payload.upload_id === uploadId.current) setProgress(event.payload);
    });
    return () => { void subscription.then(unlisten => unlisten()); };
  }, []);

  useEffect(() => {
    if (!prepared || !canvas.current) return;
    let index = 0;
    let timer: number | undefined;
    function show() {
      if (!canvas.current || !prepared) return;
      const frame = prepared.animation.frames[index];
      renderFrame(canvas.current, frame);
      index = (index + 1) % prepared.animation.frames.length;
      if (prepared.animation.frames.length > 1) timer = window.setTimeout(show, frame.delay_ms);
    }
    show();
    return () => window.clearTimeout(timer);
  }, [prepared]);

  async function load(file: File) {
    userSelected.current=true;
    setBusy(true); setError(null); setReceipt(null); setProgress(null); setPrepared(null);
    try {
      if (file.size > 10 * 1024 * 1024) throw new Error("Choose an image no larger than 10 MB.");
      const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
      const next = await invoke<Prepared>("prepare_tft_image", { bytes });
      if (alive.current) { setPrepared(next); setName(file.name); }
    } catch (e) { if (alive.current) setError(String(e)); }
    finally { if (alive.current) setBusy(false); }
  }

  async function upload() {
    if (!prepared) return;
    const id = crypto.randomUUID();
    uploadId.current = id;
    setBusy(true); setError(null); setReceipt(null);
    const total = Math.ceil((256 + prepared.animation.frames.length * 32768) / 4096);
    setProgress({ upload_id: id, completed: 0, total });
    try {
      await invoke("upload_tft_image", { animation: prepared.animation, uploadId: id, name });
      if (alive.current) setReceipt("Uploaded to your keyboard. All image blocks and SAVE acknowledged.");
    } catch (e) { if (alive.current) setError(String(e)); }
    finally { if (alive.current) setBusy(false); }
  }

  const count = prepared?.animation.frames.length ?? 0;
  return <div className="space-y-6">
    <PageHeader title="TFT Display" description="Put an image or animation on your keyboard’s 128 × 128 screen." />
    <ErrorBanner>{error}</ErrorBanner>
    <Card title="Choose an image">
      <p className="mb-4 text-sm text-fg-2">PNG, JPEG, WebP or GIF · up to 10 MB and 30 frames. Images are centre-cropped to a square. Transparent areas become black. Files are processed locally on this Mac.</p>
      <label className="grid gap-2 text-sm text-fg-1">Image file
        <input aria-label="TFT image file" type="file" accept="image/png,image/jpeg,image/webp,image/gif" disabled={busy} onChange={e => { const file = e.target.files?.[0]; if (file) void load(file); e.target.value = ""; }} />
      </label>
      <div className="mt-4"><Button size="sm" disabled={busy} onClick={() => { userSelected.current=true;setPrepared(testImage()); setName("Four-colour test image"); setError(null); setReceipt(null); setProgress(null); }}>Use test image</Button></div>
    </Card>
    <Card title="Screen preview">
      <div className="flex flex-wrap items-center gap-6">
        <canvas ref={canvas} width={128} height={128} aria-label="TFT image preview" className="h-64 w-64 rounded-md border border-line bg-black" style={{ imageRendering: "pixelated" }} />
        <div className="space-y-4 text-sm text-fg-2">
          {prepared ? <>
            <p className="font-medium text-fg-0">{name}</p>
            <p>{count === 1 ? "Static image" : `${count} animated frames · ${(prepared.animation.frames.reduce((sum, f) => sum + f.delay_ms, 0) / 1000).toFixed(2)} s per loop`}</p>
            <p>Preview uses the same RGB565 colours sent to the screen.</p>
            {prepared.timing_adjusted && <p>Frame timing adjusted to the screen’s 2–510 ms range and 2 ms steps. GIF frames with no delay use 100 ms.</p>}
          </> : <p>Choose a file or use the test image to preview it here.</p>}
          <Button variant="primary" disabled={!prepared || busy} onClick={() => void upload()}>{busy ? "Working…" : "Upload to keyboard"}</Button>
          <p>The upload replaces the keyboard’s custom image. Keep the keyboard connected until it finishes.</p>
        </div>
      </div>
      {progress && <div className="mt-5 space-y-2" aria-live="polite"><progress className="w-full accent-violet-500" max={progress.total} value={progress.completed} /><p className="text-sm text-fg-2">{progress.completed} / {progress.total} image blocks acknowledged</p></div>}
      {receipt && <p className="mt-4 text-sm text-good" role="status">{receipt}</p>}
    </Card>
  </div>;
}
