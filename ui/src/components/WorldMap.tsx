import { useEffect, useId, useRef } from "react";
import type { Key, Lang } from "../i18n";
import { WORLD_MAP_ID } from "../packs";
import { usePacks } from "../usePacks";
import { EntryTile } from "./PackViews";
import RichText from "./RichText";

type T = (k: Key) => string;

/**
 * The world map on the Maps screen, beside the Zaklon map: towns, streets
 * and buildings of the whole world, downloaded once. Where it is offered,
 * updated when a newer build is out (on the laptop), and followed while it
 * downloads. Removing it is in Settings › Storage & Downloads. Opening this
 * lets the hub look for a newer build. `open` (#maps/world) brings it into
 * view.
 */
export default function WorldMap({ t, lang, isHub, open }: { t: T; lang: Lang; isHub: boolean; open: boolean }) {
  const packs = usePacks(t, lang, isHub, { worldCheck: true });
  const pack = packs.data?.packs.find((p) => p.id === WORLD_MAP_ID);
  const box = useRef<HTMLElement>(null);
  const id = useId();
  const shown = !!pack;
  useEffect(() => {
    if (!open || !shown) return;
    box.current?.scrollIntoView({ block: "start" });
    box.current?.focus({ preventScroll: true });
  }, [open, shown]);
  // The catalog offers no world map (its checksum is not known yet): nothing to show.
  if (!pack) return packs.err ? <p className="error" role="alert">{packs.err}</p> : null;
  const e = packs.packEntry(pack, { removable: false });
  return (
    <section ref={box} id="world-map" className="panel left world-map" aria-labelledby={id} tabIndex={-1}>
      <h2 id={id}>{t("worldMapTitle")}</h2>
      <EntryTile t={t} e={e} glyph="map" as="group" />
      {packs.err && <p className="error" role="alert">{packs.err}</p>}
      <p className="muted world-map-manage">
        <RichText text={t("worldMapManage")} />
      </p>
    </section>
  );
}

/** Bring the world map into view (the link over the map, when its address is already open). */
export function showWorldMap() {
  const el = document.getElementById("world-map");
  el?.scrollIntoView({ block: "start" });
  el?.focus({ preventScroll: true });
}
