import { useEffect, useRef, useState } from "react";
import { api, type Device } from "../api";
import type { Key } from "../i18n";
import { errText } from "../errors";
import { fromText, LAPTOP, type Summary } from "../conversations";
import ConfirmButton from "./ConfirmButton";
import { Icon } from "./Icon";

type T = (k: Key) => string;
type Target = { id: string; name: string };
type Props = {
  t: T;
  conv: Summary;
  /** Away from the hub: shown, not changed. */
  readOnly: boolean;
  onRenamed: (c: Summary) => void;
  onDeleted: () => void;
};

/**
 * The open conversation's name, who sent it, and what can be done with it:
 * rename, send a copy to another device, delete. Give it the conversation's
 * id as its key, so that another conversation starts it over.
 */
export default function ChatTitle({ t, conv, readOnly, onRenamed, onDeleted }: Props) {
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState(conv.title);
  const [menu, setMenu] = useState<null | "actions" | "send">(null);
  const [targets, setTargets] = useState<Target[] | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const menuButton = useRef<HTMLButtonElement>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  const from = fromText(t, conv);

  // The menu closes on a click elsewhere or on Escape (focus goes back to its button).
  useEffect(() => {
    if (!menu) return;
    const onDown = (e: MouseEvent) => {
      if (wrap.current && !wrap.current.contains(e.target as Node)) setMenu(null);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setMenu(null);
        menuButton.current?.focus();
      }
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [menu]);

  useEffect(() => {
    if (renaming) nameInput.current?.select();
  }, [renaming]);

  const path = `/api/conversations/${encodeURIComponent(conv.id)}`;

  const rename = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    setErr(null);
    setBusy(true);
    try {
      onRenamed(await api<Summary>(path, { method: "PATCH", json: { title: name } }));
      setRenaming(false);
    } catch (ex) {
      setErr(errText(t, ex));
    } finally {
      setBusy(false);
    }
  };

  const showTargets = async () => {
    setMenu("send");
    setTargets(null);
    setErr(null);
    try {
      const [me, devices] = await Promise.all([api<{ kind: string; device?: Device }>("/api/me"), api<Device[]>("/api/devices")]);
      const list: Target[] =
        me.kind === "device"
          ? [{ id: LAPTOP, name: t("aiTheLaptop") }, ...devices.filter((d) => d.id !== me.device?.id)]
          : devices.map((d) => ({ id: d.id, name: d.name }));
      setTargets(list);
    } catch (ex) {
      setMenu(null);
      setErr(errText(t, ex));
    }
  };

  const send = async (to: Target) => {
    setErr(null);
    setBusy(true);
    try {
      await api(`${path}/send`, { json: { to: to.id } });
      setMenu(null);
      setNote(`${t("aiSentTo")} ${to.name}.`);
      menuButton.current?.focus();
    } catch (ex) {
      setErr(errText(t, ex));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    setErr(null);
    try {
      await api(path, { method: "DELETE" });
      setMenu(null);
      onDeleted();
    } catch (ex) {
      setErr(errText(t, ex));
    }
  };

  return (
    <div className="chat-title" ref={wrap}>
      <div className="chat-title-row">
        {renaming ? (
          <form className="row rename-form" onSubmit={rename}>
            <input ref={nameInput} type="text" value={name} onChange={(e) => setName(e.target.value)} aria-label={t("aiChatName")} maxLength={80} />
            <button className="btn small" disabled={busy || !name.trim()}>{t("save")}</button>
            <button type="button" className="btn secondary small" onClick={() => { setRenaming(false); setName(conv.title); }}>{t("cancel")}</button>
          </form>
        ) : (
          <h2 title={conv.title}>{conv.title || t("aiNewChat")}</h2>
        )}
        {!readOnly && !renaming && (
          <div className="menu-wrap">
            <button
              ref={menuButton}
              type="button"
              className="ghost-icon"
              aria-label={t("aiChatOptions")}
              title={t("aiChatOptions")}
              aria-expanded={menu !== null}
              onClick={() => setMenu(menu ? null : "actions")}
            >
              <Icon name="more" size={20} />
            </button>
            {menu === "actions" && (
              <div className="menu" role="group" aria-label={t("aiChatOptions")}>
                <button type="button" onClick={() => { setMenu(null); setName(conv.title); setRenaming(true); setNote(null); }}>{t("aiRename")}</button>
                <button type="button" onClick={showTargets}>{t("aiSendTo")}</button>
                <ConfirmButton label={t("aiDeleteChat")} confirmLabel={t("yesDelete")} cancelLabel={t("cancel")} className="menu-danger" onConfirm={remove} />
              </div>
            )}
            {menu === "send" && (
              <div className="menu" role="group" aria-label={t("aiSendCopy")}>
                <div className="menu-label">{t("aiSendCopy")}</div>
                {targets === null && <p className="muted menu-note">{t("aiLoading")}</p>}
                {targets?.length === 0 && <p className="muted menu-note">{t("aiSendNone")}</p>}
                {targets?.map((d) => (
                  <button type="button" key={d.id} disabled={busy} onClick={() => send(d)}>{d.name}</button>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
      {from && <div className="chat-from muted">{from}</div>}
      {note && <div className="chat-from ok" role="status">{note}</div>}
      {err && <div className="chat-from error" role="alert">{err}</div>}
    </div>
  );
}
