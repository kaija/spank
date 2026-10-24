"use client";

import { useTranslations } from "next-intl";
import { useEffect, useMemo, useRef, useState } from "react";
import { useBackend, useProfiles, useSounds } from "@/backend/BackendProvider";
import { AUDIO_EXTENSIONS } from "@/backend/types";
import type { SoundEntry } from "@/bindings/SoundEntry";
import { HeaderActions } from "@/components/SettingsShell";
import { formatDuration } from "@/lib/events";
import { profileName, soundName } from "@/lib/names";
import { usePreview } from "@/lib/usePreview";
import { Badge, Button, Sheet, type Tone } from "@/ui/controls";
import { Icon } from "@/ui/Icon";
import type { IconName } from "@/ui/iconPaths";
import s from "./LibraryPage.module.css";

type Filter = "all" | "builtin" | "user" | "pack";
/** One line in the import / export status list. */
type Job = { id: number; file: string; status: "busy" | "done" | "truncated" | "failed"; message?: string };
type Drop = { kind: "idle" } | { kind: "drag"; count: number } | { kind: "error"; ext: string };

const SOURCE_TONE: Record<SoundEntry["source"]["kind"], Tone | undefined> = {
  builtin: "neutral",
  user: undefined,
  pack: "success",
};

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;
const extOf = (path: string) => (fileName(path).match(/\.([^.]+)$/)?.[1] ?? "").toLowerCase();

/** Bars for a 64×20 thumbnail from 32 peaks in 0..1. */
export function wavePath(peaks: number[]): string {
  return peaks
    .map((p, i) => {
      const h = Math.max(0.6, Math.min(1, p) * 8);
      const x = i * 2 + 1;
      return `M${x} ${(10 - h).toFixed(1)}V${(10 + h).toFixed(1)}`;
    })
    .join("");
}

export function LibraryPage() {
  const t = useTranslations();
  const backend = useBackend();
  const [sounds, reloadSounds] = useSounds();
  const [profiles] = useProfiles();
  const preview = usePreview();

  const [filter, setFilter] = useState<Filter>("all");
  const [q, setQ] = useState("");
  const [jobs, setJobs] = useState<Job[]>([]);
  const [drop, setDrop] = useState<Drop>({ kind: "idle" });
  const [hover, setHover] = useState(false);
  const [menu, setMenu] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameVal, setRenameVal] = useState("");
  const [toDelete, setToDelete] = useState<SoundEntry | null>(null);
  const [peaks, setPeaks] = useState<Record<string, number[]>>({});
  const nextJob = useRef(0);
  const dropTimer = useRef<ReturnType<typeof setTimeout>>(undefined);

  const usedBy = useMemo(() => {
    const m = new Map<string, string[]>();
    for (const p of profiles ?? []) {
      const ids = new Set(Object.values(p.mapping).flatMap((b) => b?.sounds ?? []));
      for (const id of ids) m.set(id, [...(m.get(id) ?? []), profileName(t, p)]);
    }
    return m;
  }, [profiles, t]);

  // Waveform thumbnails, fetched once per sound.
  useEffect(() => {
    let alive = true;
    for (const snd of sounds ?? []) {
      if (peaks[snd.id]) continue;
      void backend
        .soundPeaks(snd.id)
        .then((p) => alive && setPeaks((old) => ({ ...old, [snd.id]: p })))
        .catch(() => {});
    }
    return () => {
      alive = false;
    };
  }, [sounds, backend, peaks]);

  const addJob = (job: Omit<Job, "id">) => {
    const id = ++nextJob.current;
    setJobs((js) => [...js, { ...job, id } as Job]);
    return id;
  };
  const setJob = (id: number, job: Omit<Job, "id">) =>
    setJobs((js) => js.map((j) => (j.id === id ? ({ ...job, id } as Job) : j)));

  const importFiles = async (paths: string[]) => {
    for (const path of paths) {
      const file = fileName(path);
      const id = addJob({ file, status: "busy" });
      try {
        const r = await backend.importSound(path);
        if (r.status === "added") {
          setJob(
            id,
            r.truncated
              ? { file, status: "truncated", message: t("library.truncated", { secs: (r.original_ms / 1000).toFixed(1) }) }
              : { file, status: "done", message: t("library.added") },
          );
        } else {
          const msg =
            r.reason === "too_large"
              ? t("library.tooLarge")
              : r.reason === "unsupported"
                ? t("library.unsupported")
                : r.reason === "silent"
                  ? t("library.silent")
                  : t("library.failed", { message: r.message });
          setJob(id, { file, status: "failed", message: msg });
        }
      } catch (e) {
        setJob(id, { file, status: "failed", message: t("library.failed", { message: String(e) }) });
      }
    }
    await reloadSounds();
  };

  const accept = (paths: string[]) => {
    const ok = paths.filter((p) => AUDIO_EXTENSIONS.includes(extOf(p)));
    const bad = paths.filter((p) => !ok.includes(p));
    if (bad.length) {
      clearTimeout(dropTimer.current);
      setDrop({ kind: "error", ext: extOf(bad[0]) || "?" });
      dropTimer.current = setTimeout(() => setDrop({ kind: "idle" }), 3500);
    } else {
      setDrop({ kind: "idle" });
    }
    if (ok.length) void importFiles(ok);
  };

  const acceptRef = useRef(accept);
  acceptRef.current = accept;
  // Files dropped onto the window (Tauri gives real paths).
  useEffect(
    () =>
      backend.onFileDrop((d) => {
        if (d.type === "over") setDrop({ kind: "drag", count: d.count });
        else if (d.type === "leave") setDrop({ kind: "idle" });
        else acceptRef.current(d.paths);
      }),
    [backend],
  );
  useEffect(() => () => clearTimeout(dropTimer.current), []);
  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest("[data-row-menu]")) setMenu(null);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [menu]);

  const pickFiles = async () => {
    const paths = await backend.pickAudioFiles();
    if (paths.length) accept(paths);
  };

  const importPack = async () => {
    const path = await backend.pickPack();
    if (!path) return;
    const file = fileName(path);
    const id = addJob({ file, status: "busy" });
    try {
      const r = await backend.importPack(path);
      setJob(id, {
        file,
        status: "done",
        message: r.profile
          ? t("library.packImportedProfile", { sounds: r.sounds, profile: r.profile.name })
          : t("library.packImported", { sounds: r.sounds }),
      });
    } catch (e) {
      setJob(id, { file, status: "failed", message: t("library.packFailed", { message: String(e) }) });
    }
    await reloadSounds();
  };

  const exportActive = async () => {
    const [cfg, list] = [await backend.getConfig(), profiles ?? []];
    const p = list.find((x) => x.id === cfg.active_profile);
    if (!p) return;
    const name = `${profileName(t, p)}.spankpack`;
    const path = await backend.pickPackSavePath(name);
    if (!path) return;
    const id = addJob({ file: fileName(path), status: "busy" });
    try {
      await backend.exportPack(p.id, path);
      setJob(id, { file: fileName(path), status: "done", message: t("library.exported") });
    } catch (e) {
      setJob(id, { file: fileName(path), status: "failed", message: String(e) });
    }
  };

  const commitRename = async () => {
    const id = renaming;
    setRenaming(null);
    const v = renameVal.trim();
    if (!id || !v) return;
    await backend.renameSound(id, v).catch(console.error);
    await reloadSounds();
  };

  const remove = async (snd: SoundEntry) => {
    setToDelete(null);
    await backend.deleteSound(snd.id).catch(console.error);
    await reloadSounds();
  };

  if (!sounds) return null;
  const shown = sounds.filter(
    (x) =>
      (filter === "all" || x.source.kind === filter) &&
      (!q || soundName(t, x).toLowerCase().includes(q.trim().toLowerCase())),
  );
  const srcLabel = { builtin: t("library.fromBuiltin"), user: t("library.fromUser"), pack: t("library.fromPack") };

  const dz =
    drop.kind === "drag"
      ? { state: "drag", icon: "download" as IconName, title: t("library.dropOver", { count: drop.count }), sub: t("library.dropOverSub") }
      : drop.kind === "error"
        ? { state: "error", icon: "x" as IconName, title: t("library.badFormat", { ext: drop.ext }), sub: t("library.badFormatSub") }
        : { state: hover ? "hover" : "idle", icon: "upload" as IconName, title: t("library.dropTitle"), sub: t("library.dropSub") };

  const jobView = (j: Job): { icon: IconName; color: string; msg: string } => {
    const msg = j.message ?? "";
    switch (j.status) {
      case "busy":
        return { icon: "refresh", color: "var(--color-accent)", msg: msg || t("library.importing") };
      case "done":
        return { icon: "check", color: "var(--color-success)", msg };
      case "truncated":
        return { icon: "scissors", color: "var(--color-warning)", msg };
      case "failed":
        return { icon: "warn", color: "var(--color-danger)", msg };
    }
  };

  const used = toDelete ? (usedBy.get(toDelete.id) ?? []) : [];

  return (
    <div className={s.page}>
      <HeaderActions>
        <Button onClick={() => void importPack()}>{t("library.importPack")}</Button>
        <Button onClick={() => void exportActive()}>{t("library.export")}</Button>
      </HeaderActions>

      <div
        role="button"
        tabIndex={0}
        aria-label={t("library.dropzone")}
        className={s.dropzone}
        data-state={dz.state}
        onClick={() => void pickFiles()}
        onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && void pickFiles()}
        onMouseEnter={() => setHover(true)}
        onMouseLeave={() => setHover(false)}
      >
        <span className={s.dzIcon}>
          <Icon name={dz.icon} size={18} strokeWidth={dz.icon === "x" ? 2 : 1.75} />
        </span>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div className={s.dzTitle}>{dz.title}</div>
          <div className={s.dzSub}>{dz.sub}</div>
        </div>
        <Button
          onClick={(e) => {
            e.stopPropagation();
            void pickFiles();
          }}
        >
          {t("library.chooseFiles")}
        </Button>
      </div>

      {jobs.length > 0 && (
        <div role="status" aria-live="polite" className={s.jobs}>
          {jobs.map((j) => {
            const v = jobView(j);
            return (
              <div key={j.id} className={s.job}>
                <span style={{ color: v.color, display: "flex" }}>
                  <Icon name={v.icon} size={15} strokeWidth={v.icon === "check" ? 2.2 : 1.75} />
                </span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div className={s.jobLine}>
                    <span className={s.jobFile}>{j.file}</span>
                    <span style={{ fontSize: 12, color: v.color }}>{v.msg}</span>
                  </div>
                  {j.status === "busy" && (
                    <div className={s.progress}>
                      <div className={s.progressBar} />
                    </div>
                  )}
                </div>
                <button
                  type="button"
                  className={s.dismiss}
                  aria-label={t("library.dismiss")}
                  onClick={() => setJobs((js) => js.filter((x) => x.id !== j.id))}
                >
                  <Icon name="x" size={14} />
                </button>
              </div>
            );
          })}
        </div>
      )}

      {sounds.length > 0 ? (
        <>
          <div className={s.filters}>
            <div role="tablist" aria-label={t("library.filter")} className={s.tabs}>
              {(["all", "builtin", "user", "pack"] as const).map((f) => (
                <button
                  key={f}
                  type="button"
                  role="tab"
                  aria-selected={filter === f}
                  className={s.tab}
                  onClick={() => setFilter(f)}
                >
                  {f === "all" ? t("library.all") : srcLabel[f]}
                </button>
              ))}
            </div>
            <div className={s.search}>
              <Icon name="search" size={13} style={{ position: "absolute", left: 10, color: "var(--text-faint)" }} />
              <input
                type="search"
                aria-label={t("library.search")}
                placeholder={t("library.search")}
                value={q}
                onChange={(e) => setQ(e.target.value)}
              />
            </div>
          </div>

          <div role="table" aria-label={t("library.table")} className={s.table}>
            <div role="row" className={`${s.row} ${s.head}`}>
              <span />
              <span role="columnheader">{t("library.name")}</span>
              <span role="columnheader">{t("library.wave")}</span>
              <span role="columnheader">{t("library.length")}</span>
              <span role="columnheader">{t("library.source")}</span>
              <span role="columnheader">{t("library.usedBy")}</span>
              <span />
            </div>
            {shown.map((snd) => {
              const name = soundName(t, snd);
              const playing = preview.playing === snd.id;
              const users = usedBy.get(snd.id) ?? [];
              const editable = snd.source.kind !== "builtin";
              return (
                <div key={snd.id} role="row" className={s.row} data-playing={playing ? "" : undefined}>
                  <button
                    type="button"
                    className={s.play}
                    aria-label={t(playing ? "common.stop" : "common.play", { name })}
                    onClick={() => preview.toggle(snd.id, snd)}
                  >
                    <Icon name={playing ? "stop" : "play"} size={11} strokeWidth={2} />
                  </button>
                  {renaming === snd.id ? (
                    <input
                      className={s.renameInput}
                      aria-label={t("library.renameSound")}
                      value={renameVal}
                      autoFocus
                      onChange={(e) => setRenameVal(e.target.value)}
                      onBlur={() => void commitRename()}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void commitRename();
                        if (e.key === "Escape") setRenaming(null);
                      }}
                    />
                  ) : (
                    <span
                      role="cell"
                      className={s.name}
                      onDoubleClick={() => {
                        if (!editable) return;
                        setRenaming(snd.id);
                        setRenameVal(snd.name);
                      }}
                    >
                      {name}
                    </span>
                  )}
                  <svg viewBox="0 0 64 20" width="64" height="20" aria-hidden="true" className={s.wave}>
                    <path d={wavePath(peaks[snd.id] ?? [])} />
                  </svg>
                  <span className={s.dur}>{formatDuration(snd.duration_ms)}</span>
                  <span style={{ display: "flex" }}>
                    <Badge tone={SOURCE_TONE[snd.source.kind]}>{srcLabel[snd.source.kind]}</Badge>
                  </span>
                  <span className={s.used} data-unused={users.length ? undefined : ""}>
                    {users.length ? users.join("、") : t("library.unused")}
                  </span>
                  <div style={{ position: "relative" }} data-row-menu="">
                    <button
                      type="button"
                      className={s.more}
                      aria-label={t("library.more", { name })}
                      aria-haspopup="menu"
                      aria-expanded={menu === snd.id}
                      onClick={() => setMenu(menu === snd.id ? null : snd.id)}
                    >
                      <Icon name="more" size={16} strokeWidth={2.2} />
                    </button>
                    {menu === snd.id && (
                      <div role="menu" className={s.menu}>
                        <button
                          type="button"
                          role="menuitem"
                          onClick={() => {
                            setMenu(null);
                            preview.toggle(snd.id, snd);
                          }}
                        >
                          {t("library.preview")}
                        </button>
                        {editable && (
                          <button
                            type="button"
                            role="menuitem"
                            onClick={() => {
                              setMenu(null);
                              setRenaming(snd.id);
                              setRenameVal(snd.name);
                            }}
                          >
                            {t("library.rename")}
                          </button>
                        )}
                        {editable && (
                          <button
                            type="button"
                            role="menuitem"
                            onClick={() => {
                              setMenu(null);
                              void backend.revealSound(snd.id);
                            }}
                          >
                            {t("library.reveal")}
                          </button>
                        )}
                        {editable && (
                          <>
                            <div className={s.menuSep} />
                            <button
                              type="button"
                              role="menuitem"
                              className={s.menuDanger}
                              onClick={() => {
                                setMenu(null);
                                setToDelete(snd);
                              }}
                            >
                              {t("library.deleteEllipsis")}
                            </button>
                          </>
                        )}
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
            {shown.length === 0 && <div className={s.noResults}>{t("library.noResults", { q })}</div>}
          </div>
          <p className={s.footnote}>{t("library.footer")}</p>
        </>
      ) : (
        <div className={s.emptyState}>
          <h3>{t("library.emptyTitle")}</h3>
          <p>{t("library.emptyBody")}</p>
          <Button variant="primary" onClick={() => void pickFiles()}>
            {t("library.chooseFiles")}
          </Button>
        </div>
      )}

      {toDelete && (
        <Sheet
          title={t(used.length ? "library.deleteUsedTitle" : "library.deleteTitle", { name: soundName(t, toDelete) })}
          body={t(used.length ? "library.deleteUsedBody" : "library.deleteBody")}
          extra={
            used.length > 0 && (
              <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginTop: 10 }}>
                {used.map((u) => (
                  <Badge key={u} tone="warning">
                    {u}
                  </Badge>
                ))}
              </div>
            )
          }
          confirmLabel={used.length ? t("library.deleteAnyway") : t("common.delete")}
          cancelLabel={t("common.cancel")}
          onCancel={() => setToDelete(null)}
          onConfirm={() => void remove(toDelete)}
        />
      )}
    </div>
  );
}
