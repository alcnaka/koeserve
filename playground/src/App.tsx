import { FormEvent, useEffect, useMemo, useState } from "react";
import { api } from "./api/client";
import type { components } from "./api/schema";

type VoiceInfo = components["schemas"]["VoiceInfo"];
type SynthesisRequest = components["schemas"]["SynthesisRequest"];

export default function App() {
  const [voices, setVoices] = useState<VoiceInfo[]>([]);
  const [voiceId, setVoiceId] = useState("");
  const [text, setText] = useState("こんにちは。音声合成テストです。");
  const [speed, setSpeed] = useState(1);
  const [pitch, setPitch] = useState(0);
  const [alpha, setAlpha] = useState(0.42);
  const [useModelAlpha, setUseModelAlpha] = useState(true);
  const [beta, setBeta] = useState(0);
  const [volumeDb, setVolumeDb] = useState(0);
  const [audioUrl, setAudioUrl] = useState<string>();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string>();

  const selectedVoice = useMemo(
    () => voices.find((voice) => voice.id === voiceId),
    [voices, voiceId]
  );

  useEffect(() => {
    void (async () => {
      const { data, error } = await api.GET("/v1/voices");
      if (error || !data) {
        setError("ボイス一覧を取得できませんでした。");
        return;
      }
      setVoices(data);
      if (data.length > 0) setVoiceId(data[0].id);
    })();
  }, []);

  useEffect(() => {
    if (!selectedVoice) return;
    const defaults = selectedVoice.defaults;
    setSpeed(defaults.speed);
    setPitch(defaults.pitch);
    setAlpha(defaults.alpha);
    setUseModelAlpha(true);
    setBeta(defaults.beta);
    setVolumeDb(defaults.volume_db);
  }, [selectedVoice]);

  useEffect(() => {
    return () => {
      if (audioUrl) URL.revokeObjectURL(audioUrl);
    };
  }, [audioUrl]);

  async function synthesize(event: FormEvent) {
    event.preventDefault();
    if (!voiceId || !text.trim()) return;

    setLoading(true);
    setError(undefined);

    const body: SynthesisRequest = {
      text: text.trim(),
      voice: voiceId,
      speed,
      pitch,
      alpha: useModelAlpha ? null : alpha,
      beta,
      volume_db: volumeDb
    };

    try {
      const { data, error, response } = await api.POST("/v1/speech", {
        body,
        parseAs: "blob"
      });

      if (error || !data || !response.ok) {
        setError(`音声生成に失敗しました (${response.status})`);
        return;
      }

      if (audioUrl) URL.revokeObjectURL(audioUrl);
      setAudioUrl(URL.createObjectURL(data as Blob));
    } catch (err) {
      setError(err instanceof Error ? err.message : "音声生成に失敗しました。");
    } finally {
      setLoading(false);
    }
  }

  return (
    <main className="shell">
      <section className="card">
        <header>
          <p className="eyebrow">KoeServe</p>
          <h1>Playground</h1>
          <p className="muted">HTS voice と合成パラメーターをブラウザから試せます。</p>
        </header>

        <form onSubmit={synthesize}>
          <label>
            <span>Voice</span>
            <select value={voiceId} onChange={(e) => setVoiceId(e.target.value)} disabled={voices.length === 0}>
              {voices.map((voice) => (
                <option key={voice.id} value={voice.id}>
                  {voice.display_name} ({voice.id})
                </option>
              ))}
            </select>
          </label>

          <label>
            <span>Text</span>
            <textarea value={text} onChange={(e) => setText(e.target.value)} maxLength={500} rows={6} />
            <small>{text.length} / 500</small>
          </label>

          <div className="controls">
            <Slider label="Speed" min={0.5} max={2} step={0.01} value={speed} onChange={setSpeed} suffix="x" />
            <Slider label="Pitch" min={-12} max={12} step={0.1} value={pitch} onChange={setPitch} suffix=" st" />
            <div className="parameter">
              <div className="parameter-head">
                <span>Alpha</span>
                <strong>{useModelAlpha ? "Model default" : alpha.toFixed(2)}</strong>
              </div>
              <input type="range" min={0} max={1} step={0.01} value={alpha} disabled={useModelAlpha} onChange={(e) => setAlpha(Number(e.target.value))} />
              <label className="check"><input type="checkbox" checked={useModelAlpha} onChange={(e) => setUseModelAlpha(e.target.checked)} /> Use model default</label>
            </div>
            <Slider label="Beta" min={0} max={1} step={0.01} value={beta} onChange={setBeta} />
            <Slider label="Volume" min={-40} max={20} step={0.5} value={volumeDb} onChange={setVolumeDb} suffix=" dB" />
          </div>

          <button type="submit" disabled={loading || !voiceId || !text.trim()}>
            {loading ? "Generating…" : "Generate"}
          </button>
        </form>

        {error && <p className="error">{error}</p>}
        {audioUrl && (
          <section className="player">
            <audio src={audioUrl} controls autoPlay />
            <a href={audioUrl} download={`${voiceId || "speech"}.wav`}>Download WAV</a>
          </section>
        )}
      </section>
    </main>
  );
}

function Slider(props: {
  label: string;
  min: number;
  max: number;
  step: number;
  value: number;
  onChange: (value: number) => void;
  suffix?: string;
}) {
  return (
    <label className="parameter">
      <div className="parameter-head"><span>{props.label}</span><strong>{props.value}{props.suffix ?? ""}</strong></div>
      <input type="range" min={props.min} max={props.max} step={props.step} value={props.value} onChange={(e) => props.onChange(Number(e.target.value))} />
    </label>
  );
}
