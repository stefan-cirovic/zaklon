import type { ReactNode } from "react";
import type { Key } from "../i18n";
import { fromText, groupByRecency, type Summary } from "../conversations";
import { Icon } from "./Icon";

type T = (k: Key) => string;
type Props = {
  t: T;
  /** Above the rest: the screen's heading on a wide screen. */
  head?: ReactNode;
  /** The conversations to show (a search's results while one is typed); null while loading. */
  list: Summary[] | null;
  query: string;
  setQuery: (q: string) => void;
  openId: string | null;
  open: (id: string) => void;
  newChat: () => void;
  showMemory: () => void;
};

/** The saved conversations of this device: "New conversation", a search, and the list by recency. */
export default function ConversationList({ t, head, list, query, setQuery, openId, open, newChat, showMemory }: Props) {
  const groups = list ? groupByRecency(list) : [];
  return (
    <div className="convo-side-inner">
      <div className="convo-side-head">
        {head}
        <button type="button" className="btn secondary new-chat" onClick={newChat}>
          <Icon name="plus" size={18} />
          {t("aiNewChat")}
        </button>
        <input
          type="search"
          className="convo-search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("aiSearchChats")}
          aria-label={t("aiSearchChats")}
          maxLength={200}
        />
      </div>
      <nav className="convo-list" aria-label={t("aiConversations")}>
        {list && list.length === 0 && <p className="muted convo-empty">{query.trim() ? t("aiNoChatsFound") : t("aiNoChats")}</p>}
        {groups.map((g) => (
          <section key={g.key} className="convo-group">
            <h3>{t(g.key)}</h3>
            <ul>
              {g.items.map((c) => {
                const from = fromText(t, c);
                const title = c.title || t("aiNewChat");
                return (
                  <li key={c.id}>
                    <button
                      type="button"
                      className={"convo-item" + (c.id === openId ? " active" : "")}
                      aria-current={c.id === openId ? "true" : undefined}
                      aria-label={from ? `${title}, ${from}` : undefined}
                      onClick={() => open(c.id)}
                      title={from ? `${title} · ${from}` : title}
                    >
                      {from && (
                        <span className="convo-from">
                          <Icon name="shared" size={14} />
                        </span>
                      )}
                      <span className="convo-title">{title}</span>
                    </button>
                  </li>
                );
              })}
            </ul>
          </section>
        ))}
      </nav>
      <div className="convo-side-foot">
        <button type="button" className="convo-memory" onClick={showMemory}>
          <Icon name="memory" size={18} />
          <span>{t("memoryTitle")}</span>
        </button>
      </div>
    </div>
  );
}
