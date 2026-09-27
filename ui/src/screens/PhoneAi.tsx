import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { fmtBytes, fmtQty } from "../format";
import ConfirmButton from "../components/ConfirmButton";

type T = (k: Key) => string;
type Status = {
  engine: boolean;
  models: { file: string; size: number }[];
  running: string | null;
  starting: boolean;
  copy: { model: string; model_id: string; verifying: boolean; done: number; total: number; error: string | null; finished: boolean } | null;
  cpu_cores: number;
};
type HubModel = { id: string; title_en: string; title_sr: string; file: string; size: number; sha256: string };
type Answer = { text: string; tokens: number; tokens_per_second: number; prompt_ms: number; total_ms: number };

/**
 * On-device AI on the phone: copy a model from the hub, start the engine
 * that ships in the app, and ask it something. Works without the hub once
 * the model is on the phone.
 */
export default function PhoneAi({ t, lang }: { t: T; lang: Lang }) {
  const [st, setSt] = useState<Status | null>(null);
  const [hubModels, setHubModels] = useState<HubModel[] | null>(null);
  const [prompt, setPrompt] = useState("");
  const [answer, setAnswer] = useState<Answer | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setSt(await invoke<Status>("local_ai_status"));
    } catch (e) {
      setErr(errText(t, e));
    }
  }, [t]);

  useEffect(() => {
    load();
    api<HubModel[]>("/api/models").then(setHubModels).catch(() => setHubModels([]));
  }, [load]);

  // Poll while something is in progress (copying or starting).
  const active = !!st && (st.starting || (!!st.copy && !st.copy.finished));
  useEffect(() => {
    if (!active) return;
    const id = setInterval(load, 1000);
    return () => clearInterval(id);
  }, [active, load]);

  // A locked phone pauses the copy. Keep the screen on while a model is being
  // copied (the Screen Wake Lock of the web view); it is let go when done.
  const copying = !!st?.copy && !st.copy.finished;
  useEffect(() => {
    if (!copying) return;
    type Sentinel = { release: () => Promise<void> };
    const wl = (navigator as unknown as { wakeLock?: { request: (t: "screen") => Promise<Sentinel> } }).wakeLock;
    if (!wl) return;
    let sentinel: Sentinel | null = null;
    let alive = true;
    const take = () => {
      if (document.visibilityState !== "visible") return;
      wl.request("screen")
        .then((s) => {
          if (alive) sentinel = s;
          else s.release().catch(() => {});
        })
        .catch(() => {});
    };
    take();
    // The lock ends when the app goes to the background; take it again on return.
    document.addEventListener("visibilitychange", take);
    return () => {
      alive = false;
      document.removeEventListener("visibilitychange", take);
      sentinel?.release().catch(() => {});
    };
  }, [copying]);

  // The phone pauses network work while the screen is locked. When the app
  // comes back after an interrupted copy, carry on by itself.
  useEffect(() => {
    const onVisible = () => {
      if (document.visibilityState !== "visible" || !st?.copy) return;
      const c = st.copy;
      if (c.finished && c.error && /connection lost|not reachable|no answer|timed out/i.test(c.error)) {
        invoke("local_ai_copy", { modelId: c.model_id, file: c.model }).then(load).catch(() => {});
      }
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => document.removeEventListener("visibilitychange", onVisible);
  }, [st, load]);

  const copy = async (m: HubModel) => {
    setErr(null);
    try {
      await invoke("local_ai_copy", { modelId: m.id, file: m.file });
      load();
    } catch (e) {
      setErr(errText(t, e));
    }
  };

  const start = async (file: string) => {
    setErr(null);
    setBusy(true);
    const poll = setInterval(load, 1000);
    try {
      await invoke("local_ai_start", { file });
    } catch (e) {
      setErr(errText(t, e));
    } finally {
      clearInterval(poll);
      setBusy(false);
      load();
    }
  };

  const ask = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!prompt.trim()) return;
    setErr(null);
    setBusy(true);
    setAnswer(null);
    try {
      setAnswer(await invoke<Answer>("local_ai_ask", { prompt, language: lang }));
    } catch (ex) {
      setErr(errText(t, ex));
    } finally {
      setBusy(false);
    }
  };

  if (!st) return <p className="muted">…</p>;
  if (!st.engine) return <p className="warn">{t("aiEngineMissing")}</p>;

  const onPhone = new Set(st.models.map((m) => m.file));
  const title = (m: HubModel) => (lang === "sr" && m.title_sr ? m.title_sr : m.title_en);

  return (
    <div className="stack">
      <p className="muted">{t("phoneAiIntro")}</p>
      {err && <p className="error" role="alert">{err}</p>}

      <div>
        <h2>{t("modelsOnPhone")}</h2>
        {st.models.length === 0 ? (
          <p className="muted">{t("noModelsOnPhone")}</p>
        ) : (
          <div className="list">
            {st.models.map((m) => (
              <div className="item wrap" key={m.file}>
                <div>
                  <div>{m.file.replace(".gguf", "")}</div>
                  <div className="muted" style={{ fontSize: 13 }}>
                    {fmtBytes(m.size)}
                    {st.running === m.file && <span className="ok"> · {t("aiRunning")}</span>}
                  </div>
                </div>
                <div className="row">
                  {st.running === m.file ? (
                    <button className="btn secondary" onClick={() => invoke("local_ai_stop").then(load)}>{t("aiStop")}</button>
                  ) : (
                    <button className="btn" onClick={() => start(m.file)} disabled={busy}>
                      {st.starting ? t("aiLoading") : t("aiStart")}
                    </button>
                  )}
                  <ConfirmButton label={t("delete")} confirmLabel={t("yesDelete")} cancelLabel={t("cancel")} onConfirm={() => invoke("local_ai_delete", { file: m.file }).then(load)} />
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {st.copy && !st.copy.finished && (
        <div className="panel">
          <div className="label">{st.copy.verifying ? t("copyVerifying") : t("copyingModel")}</div>
          <div className="bar"><i style={{ width: `${st.copy.total ? Math.round((st.copy.done / st.copy.total) * 100) : 0}%` }} /></div>
          <div className="muted" style={{ fontSize: 13 }}>{fmtBytes(st.copy.done)} / {fmtBytes(st.copy.total)}</div>
          <div className="muted" style={{ fontSize: 13 }}>{t("keepScreenOn")}</div>
        </div>
      )}
      {st.copy?.finished && st.copy.error && <p className="error" role="alert">{errText(t, new Error(st.copy.error))}</p>}

      <div>
        <h2>{t("modelsOnHub")}</h2>
        {hubModels === null ? (
          <p className="muted">…</p>
        ) : hubModels.length === 0 ? (
          <p className="muted">{t("noModelsOnHub")}</p>
        ) : (
          <div className="list">
            {hubModels.map((m) => (
              <div className="item wrap" key={m.id}>
                <div>
                  <div>{title(m)}</div>
                  <div className="muted" style={{ fontSize: 13 }}>{fmtBytes(m.size)}</div>
                </div>
                {onPhone.has(m.file) ? (
                  <span className="ok">{t("installed")}</span>
                ) : (
                  <button className="btn secondary" onClick={() => copy(m)} disabled={active}>{t("copyToPhone")}</button>
                )}
              </div>
            ))}
          </div>
        )}
      </div>

      {st.running && (
        <form className="stack panel" onSubmit={ask}>
          <label className="field">
            {t("askSomething")}
            <textarea value={prompt} onChange={(e) => setPrompt(e.target.value)} rows={3} maxLength={2000} placeholder={t("askExample")} />
          </label>
          <div className="row actions">
            <button className="btn" disabled={busy || !prompt.trim()}>{busy ? t("aiThinking") : t("ask")}</button>
          </div>
          {answer && (
            <div className="stack">
              <div className="answer">{answer.text}</div>
              <div className="muted" style={{ fontSize: 13 }}>
                {fmtQty(Math.round(answer.tokens_per_second * 10) / 10)} {t("tokensPerSecond")} · {fmtQty(Math.round(answer.total_ms / 100) / 10)} s
              </div>
            </div>
          )}
        </form>
      )}
    </div>
  );
}
