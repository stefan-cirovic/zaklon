import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { Key } from "../i18n";
import { errText } from "../errors";
import ConfirmButton from "./ConfirmButton";

type T = (k: Key) => string;
type Note = { id: string; text: string; created_at: string; created_by: string | null };

/** What the household asked the assistant to remember: seen, added and deleted by anyone. */
export default function Memory({ t, version }: { t: T; version: number }) {
  const [notes, setNotes] = useState<Note[] | null>(null);
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(() => {
    api<Note[]>("/api/memory").then(setNotes).catch(() => {});
  }, []);
  useEffect(load, [load, version]);

  const add = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!text.trim()) return;
    setErr(null);
    try {
      await api("/api/memory", { json: { text } });
      setText("");
      load();
    } catch (ex) {
      setErr(errText(t, ex));
    }
  };

  const remove = async (id: string) => {
    try {
      await api(`/api/memory/${encodeURIComponent(id)}`, { method: "DELETE" });
      load();
    } catch (ex) {
      setErr(errText(t, ex));
    }
  };

  if (!notes) return null;
  return (
    <div className="stack memory">
      <button className="btn secondary" onClick={() => setOpen(!open)} aria-expanded={open}>
        {open ? "▴ " : "▾ "}
        {t("memoryTitle")} ({notes.length})
      </button>
      {open && (
        <div className="panel stack left">
          <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("memoryIntro")}</p>
          {err && <p className="error" role="alert">{err}</p>}
          {notes.length === 0 && <p className="muted" style={{ margin: 0 }}>{t("memoryEmpty")}</p>}
          <div className="list">
            {notes.map((n) => (
              <div className="row between wrap memory-row" key={n.id}>
                <span>{n.text}</span>
                <ConfirmButton label={t("delete")} confirmLabel={t("yesDelete")} cancelLabel={t("cancel")} className="btn danger small" onConfirm={() => remove(n.id)} />
              </div>
            ))}
          </div>
          <form className="row" onSubmit={add}>
            <input type="text" value={text} onChange={(e) => setText(e.target.value)} placeholder={t("memoryPlaceholder")} aria-label={t("memoryAdd")} maxLength={300} />
            <button className="btn secondary" disabled={!text.trim()}>{t("memoryAdd")}</button>
          </form>
        </div>
      )}
    </div>
  );
}
