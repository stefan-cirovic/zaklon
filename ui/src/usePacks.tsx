import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import type { Key, Lang } from "./i18n";
import { errText } from "./errors";
import { useVisiblePoll } from "./poll";
import { fmtBytes, fmtDate } from "./format";
import ConfirmButton from "./components/ConfirmButton";
import { LicenseAsk } from "./components/PackViews";
import { buildDay, BUSY, downloadedByUser, packKind, WITH_BAR, WORLD_MAP_ID, type CatalogReply, type Entry, type Localized, type Pack } from "./packs";
import { countryState, type Country, type MapsReply } from "./maps";
import { packTopics } from "./topics";

type T = (k: Key) => string;

export type PacksOptions = {
  /** Also the world's maps for phones (/api/maps). */
  maps?: boolean;
  /** Let the hub look for a newer world map: the screen offers the world map. */
  worldCheck?: boolean;
  /** Mark the AI models this computer has no memory for: the screen lists models. */
  models?: boolean;
};

/**
 * The packs as every screen that offers or manages them shows them: the
 * catalog (asked for again every 10 s, every 1.5 s while something
 * downloads), optionally the world's maps, and for each pack an Entry with
 * its state and its buttons. One pack's license question at a time is open.
 * `removable` entries have Remove (on the laptop): only where things are
 * managed, Storage & Downloads and the AI models in Settings.
 */
export function usePacks(t: T, lang: Lang, isHub: boolean, { maps: withMaps = false, worldCheck = false, models = false }: PacksOptions = {}) {
  const [data, setData] = useState<CatalogReply | null>(null);
  const [maps, setMaps] = useState<MapsReply | null>(null);
  // The AI models the hub does not have the memory for (they run on the hub).
  const [tooBig, setTooBig] = useState<Set<string>>(new Set());
  const [err, setErr] = useState<string | null>(null);
  // The pack whose question is open (its license, or removing the old world map first).
  const [asking, setAsking] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setData(await api<CatalogReply>("/api/catalog"));
      setErr(null);
      return true;
    } catch (e) {
      setErr(errText(t, e));
      return false;
    }
  }, [t]);
  const busy = data?.packs.some((p) => BUSY.includes(p.state.status)) ?? false;
  useVisiblePoll(load, busy ? 1500 : 10000);

  // Where the world map is offered, the hub may look for a newer one: it reads
  // Protomaps' list of builds, at most about once a day, in the background.
  useEffect(() => {
    if (!worldCheck) return;
    let later: number | undefined;
    api<{ checking?: boolean } | undefined>("/api/world-map/check", { method: "POST" })
      .then((r) => {
        if (r?.checking) later = window.setTimeout(() => void load(), 3000);
      })
      .catch(() => {});
    return () => window.clearTimeout(later);
  }, [load, worldCheck]);

  useEffect(() => {
    if (!models) return;
    api<{ models: { id: string; fits?: boolean }[] }>("/api/assistant")
      .then((a) => setTooBig(new Set(a.models.filter((m) => m.fits === false).map((m) => m.id))))
      .catch(() => {});
  }, [models]);

  const loadMaps = useCallback(async () => {
    try {
      setMaps(await api<MapsReply>("/api/maps"));
      return true;
    } catch {
      return false;
    }
  }, []);
  const mapsBusy = maps?.countries.some((c) => countryState(c).busy) ?? false;
  const refreshMaps = useVisiblePoll(loadMaps, withMaps ? (mapsBusy ? 2000 : 15000) : null);

  const act = async (path: string, method: string, then: () => void) => {
    try {
      await api(path, { method });
      then();
    } catch (e) {
      setErr(errText(t, e));
    }
  };

  const title = (l: Localized) => (lang === "sr" && l.sr ? l.sr : l.en);
  const world = data?.world ?? null;
  const buildDate = (build: string) => fmtDate(buildDay(build));
  /** Update the world map (laptop): the hub downloads the newer build next to the old one, or removes the old one first. */
  const updateWorld = async (removeOld: boolean) => {
    try {
      await api("/api/world-map/update", { method: "POST", json: { remove_old: removeOld } });
      await load();
    } catch (e) {
      setErr(errText(t, e));
    }
  };
  const nameOf = (x: { name: string; name_sr: string }) => (lang === "sr" ? x.name_sr : x.name);
  // In the details view every button is a quiet one: a column of accent buttons would shout.
  const btn = (small: boolean, primary: boolean) => (primary && !small ? "btn" : "btn secondary") + (small ? " small" : "");

  /**
   * Why a pack is one people download themselves, in one line ("note") or as
   * the question before its download ("ask"), naming its license.
   */
  const offerText = (p: Pack, kind: "note" | "ask") => {
    const license = p.license || "?";
    const [note, ask]: [Key, Key] =
      p.offer_reason === "noncommercial"
        ? ["offerReasonNc", "offerAskNc"]
        : p.offer_reason === "mixed_licenses"
          ? ["offerReasonMixed", "offerAskMixed"]
          : ["offerReasonOther", "offerAskOther"];
    return t(kind === "note" ? note : ask).replace("{license}", license);
  };

  const packEntry = (p: Pack, { removable }: { removable: boolean }): Entry => {
    const s = p.state;
    const name = title(p.title);
    const pct = s.bytes_total ? Math.min(100, Math.round((s.bytes_done / s.bytes_total) * 100)) : 0;
    const url = `/api/packs/${encodeURIComponent(p.id)}`;
    const byUser = downloadedByUser(p);
    const withdrawn = !!p.withdrawn;
    // A pack people download themselves starts only after its license was
    // confirmed, which also covers resuming, retrying and updating it.
    const download = () => act(`${url}/download${byUser ? "?accept_license=true" : ""}`, "POST", load);
    const pause = () => act(`${url}/pause`, "POST", load);
    const ask = () => setAsking(p.id);
    const unask = () => {
      setAsking(null);
      // Back to the button that opened the question.
      requestAnimationFrame(() => document.querySelector<HTMLElement>(`[data-entry="${CSS.escape(p.id)}"] [data-asks-license]`)?.focus());
    };
    // The world map with an older build on the hub: downloading is updating
    // it, on the laptop only; without room for both maps it first asks
    // whether to remove the old one.
    const isWorld = p.id === WORLD_MAP_ID && !!world;
    const upd = isWorld && !!s.update_available;
    const startUpdate = () => (world?.room_for_both ? void updateWorld(false) : setAsking(p.id));
    let status = t("notDownloaded");
    let tone: Entry["tone"] = "muted";
    if (s.status === "queued") status = t("queued");
    else if (s.status === "downloading") status = `${t("downloadingNow")} ${pct}%`;
    else if (s.status === "verifying") status = t("verifying");
    else if (s.status === "paused") status = `${t("pausedStatus")} ${pct}%`;
    else if (s.status === "failed") {
      status = t("failedStatus");
      tone = "warn";
    } else if (s.status === "installed") {
      status = !s.update_available
        ? t("installed")
        : upd && world
          ? t("worldNewer").replace("{date}", buildDate(world.offered)).replace("{size}", fmtBytes(world.offered_size))
          : t("packUpdateAvailable");
      tone = s.update_available ? "warn" : "ok";
    }
    const remove = (small: boolean) =>
      isHub &&
      removable && (
        <ConfirmButton
          label={t("remove")}
          ariaLabel={`${t("remove")}: ${name}`}
          confirmLabel={t("yesRemove")}
          cancelLabel={t("cancel")}
          className={"btn danger" + (small ? " small" : "")}
          onConfirm={() => act(url, "DELETE", load)}
        />
      );
    const button = (small: boolean, primary: boolean, label: string, onClick: () => void) => (
      <button className={btn(small, primary)} aria-label={`${label}: ${name}`} onClick={onClick}>
        {label}
      </button>
    );
    return {
      key: p.id,
      kind: packKind(p),
      topics: p.category === "knowledge" ? packTopics(p.topics) : [],
      name,
      desc: title(p.description),
      // The world map on the hub: the size of its build there.
      size: isWorld && world?.installed ? world.installed_size : p.size,
      // The world map names its build by date: the one on the hub, else the one offered.
      meta: `${isWorld && world ? t("worldBuild").replace("{date}", buildDate(world.installed ?? world.offered)) : p.version} · ${p.license}`,
      license: p.license,
      note: withdrawn
        ? t("packWithdrawn")
        : byUser
          ? offerText(p, "note")
          : upd && (BUSY.includes(s.status) || s.status === "paused")
            ? t("worldKeptMeanwhile")
            : upd && !isHub
              ? t("worldUpdateOnLaptop")
              : null,
      byUser,
      credit: p.attribution || p.source ? { attribution: p.attribution, source: p.source ?? "" } : null,
      ask:
        asking === p.id && byUser && s.status === "not_installed" && !withdrawn ? (
          <LicenseAsk
            t={t}
            text={offerText(p, "ask")}
            name={name}
            onYes={() => {
              setAsking(null);
              download();
            }}
            onNo={unask}
          />
        ) : asking === p.id && upd && world && isHub ? (
          <LicenseAsk
            t={t}
            text={t("worldNoRoom").replace("{need}", fmtBytes(world.needed)).replace("{free}", fmtBytes(world.disk_free))}
            name={name}
            yes={t("worldRemoveOld")}
            danger
            onYes={() => {
              setAsking(null);
              void updateWorld(true);
            }}
            onNo={unask}
          />
        ) : null,
      // Nothing people download themselves is ever suggested.
      recommended: p.recommended_for.includes(lang) && !byUser && !withdrawn,
      status,
      tone,
      progress: WITH_BAR.includes(s.status) ? pct : null,
      detail:
        (s.status === "downloading" || s.status === "paused" || s.status === "queued") && s.bytes_total
          ? `${fmtBytes(s.bytes_done)} / ${fmtBytes(s.bytes_total)}${s.status === "downloading" && s.speed > 0 ? ` · ${fmtBytes(s.speed)}/s` : ""}`
          : null,
      error:
        s.status === "failed" && s.error
          ? isWorld && /no longer at its download address/.test(s.error)
            ? t("worldGone")
            : errText(t, new Error(s.error))
          : tooBig.has(p.id)
            ? t("aiModelTooBig")
            : null,
      // No longer offered: nothing to download or update, only to delete (on the laptop).
      actions: (small) =>
        withdrawn ? (
          (s.status === "paused" || s.status === "failed" || s.status === "installed") && remove(small)
        ) : upd ? (
          <>
            {s.status === "queued" && button(small, false, t("cancel"), pause)}
            {(s.status === "downloading" || s.status === "verifying") && button(small, false, t("pause"), pause)}
            {isHub && s.status === "installed" && asking !== p.id && (
              <button className={btn(small, true)} aria-label={`${t("worldUpdate")}: ${name}`} data-asks-license="" onClick={startUpdate}>
                {t("worldUpdate")}
              </button>
            )}
            {isHub && (s.status === "paused" || s.status === "failed") && asking !== p.id && button(small, true, s.status === "paused" ? t("resume") : t("retry"), startUpdate)}
            {(s.status === "paused" || s.status === "failed" || s.status === "installed") && remove(small)}
          </>
        ) : (
          <>
            {s.status === "not_installed" &&
              (byUser ? (
                asking !== p.id && (
                  <button className={btn(small, true)} aria-label={`${t("download")}: ${name}`} data-asks-license="" onClick={ask}>
                    {t("download")}
                  </button>
                )
              ) : (
                button(small, true, t("download"), download)
              ))}
            {s.status === "queued" && button(small, false, t("cancel"), pause)}
            {(s.status === "downloading" || s.status === "verifying") && button(small, false, t("pause"), pause)}
            {(s.status === "paused" || s.status === "failed") && button(small, true, s.status === "paused" ? t("resume") : t("retry"), download)}
            {s.status === "installed" && s.update_available && button(small, true, t("packUpdate"), download)}
            {(s.status === "paused" || s.status === "failed" || s.status === "installed") && remove(small)}
          </>
        ),
      onDisk: s.status === "installed" || s.bytes_done > 0 || BUSY.includes(s.status),
      installed: s.status === "installed",
      update: !!s.update_available && !withdrawn,
      installedBytes: isWorld && world?.installed ? world.installed_size : p.size,
      busy: BUSY.includes(s.status),
      stopped: s.status === "paused" || s.status === "failed",
      bytesDone: s.status === "installed" ? p.size : s.bytes_done,
      bytesTotal: s.bytes_total || p.size,
    };
  };

  /** A country's map for phones (CoMaps), its pieces summed up. */
  const countryEntry = (c: Country, { removable }: { removable: boolean }): Entry => {
    const s = countryState(c);
    const name = nameOf(c);
    const pct = c.size ? Math.min(100, Math.round((s.done / c.size) * 100)) : 0;
    const url = `/api/maps/${encodeURIComponent(c.id)}`;
    let status = t("notDownloaded");
    let tone: Entry["tone"] = "muted";
    if (s.busy) status = `${t("downloadingNow")} ${pct}%`;
    else if (s.all) {
      status = s.update ? t("packUpdateAvailable") : t("installed");
      tone = s.update ? "warn" : "ok";
    } else if (s.failed) {
      status = t("failedStatus");
      tone = "warn";
    } else if (s.paused) status = `${t("pausedStatus")} ${pct}%`;
    else if (s.some) status = `${s.installed}/${c.regions.length} ${t("installed").toLowerCase()}`;
    const label = s.update ? t("packUpdate") : s.some || s.paused ? t("resume") : s.failed ? t("retry") : t("download");
    const installedBytes = c.regions.reduce((sum, r) => sum + (r.status === "installed" ? r.size : 0), 0);
    return {
      key: `map:${c.id}`,
      kind: "maps",
      topics: [],
      name: `${t("mapsOf")}: ${name}`,
      desc: c.regions.length > 1 ? `${c.regions.length} ${t("mapsRegions")}` : "",
      size: c.size,
      meta: "ODbL-1.0",
      license: "ODbL-1.0",
      note: null,
      byUser: false,
      credit: null,
      ask: null,
      recommended: false,
      status,
      tone,
      progress: s.busy || s.paused ? pct : null,
      detail: s.busy ? `${fmtBytes(s.done)} / ${fmtBytes(c.size)}` : null,
      error: s.failed && !s.busy ? t("mapsSomeFailed") : null,
      actions: (small) => (
        <>
          {!(s.all && !s.update) && !s.busy && (
            <button className={btn(small, true)} aria-label={`${label}: ${name}`} onClick={() => act(`${url}/download`, "POST", refreshMaps)}>
              {label}
            </button>
          )}
          {isHub && removable && (s.some || s.failed || s.paused) && !s.busy && (
            <ConfirmButton
              label={t("remove")}
              ariaLabel={`${t("remove")}: ${name}`}
              confirmLabel={t("yesRemove")}
              cancelLabel={t("cancel")}
              className={"btn danger" + (small ? " small" : "")}
              onConfirm={() => act(url, "DELETE", refreshMaps)}
            />
          )}
        </>
      ),
      onDisk: s.some || s.busy || s.paused || s.done > 0,
      installed: s.some,
      update: s.update,
      installedBytes,
      busy: s.busy,
      stopped: (s.paused || s.failed) && !s.busy,
      bytesDone: s.done,
      bytesTotal: c.size,
    };
  };

  return { data, maps, err, setErr, load, refreshMaps, packEntry, countryEntry, title, nameOf };
}
