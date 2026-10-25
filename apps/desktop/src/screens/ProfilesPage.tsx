"use client";

import { useTranslations } from "next-intl";
import { useEffect, useMemo, useState, type DragEvent } from "react";
import { useBackend, useConfig, useProfiles, useSoundIndex, useSounds } from "@/backend/BackendProvider";
import type { AppConfig } from "@/bindings/AppConfig";
import type { EventKind } from "@/bindings/EventKind";
import type { Profile } from "@/bindings/Profile";
import { EVENT_ICONS, EVENT_KINDS, formatDuration } from "@/lib/events";
import { profileDesc, profileName, soundName } from "@/lib/names";
import { usePreview } from "@/lib/usePreview";
import { Badge, Button, IconButton, Sheet, Slider, Switch, Tag } from "@/ui/controls";
import { Icon } from "@/ui/Icon";
import s from "./ProfilesPage.module.css";

const DRAG_TYPE = "application/x-spank-sound";

function hint(t: ReturnType<typeof useTranslations>, k: EventKind, cfg: AppConfig | undefined): string {
  const lid = cfg?.lid;
  const slap = cfg?.slap;
  switch (k) {
    case "lid_opened":
      return t("events.hintOpened", { deg: lid?.open_min_deg ?? 10 });
    case "lid_closing":
      return t("events.hintClosing", { deg: lid?.close_threshold_deg ?? 20 });
    case "lid_overbent":
      return t("events.hintOverbent", { from: lid?.opening_start_deg ?? 30, to: lid?.overbend_threshold_deg ?? 120 });
    case "slap_light":
      return t("events.hintLight");
    case "slap_medium":
      return t("events.hintMedium");
    case "slap_hard":
      return t("events.hintHard");
    case "slap_combo":
      return t("events.hintCombo", { secs: (slap?.combo_window_ms ?? 5000) / 1000, n: slap?.combo_count ?? 3 });
  }
}

export function ProfilesPage() {
  const t = useTranslations();
  const backend = useBackend();
  const [profiles, reload, setProfiles] = useProfiles();
  const [sounds] = useSounds();
  const [config] = useConfig();
  const index = useSoundIndex(sounds);
  const preview = usePreview();

  const [selected, setSelected] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameVal, setRenameVal] = useState("");
  const [addOpen, setAddOpen] = useState<EventKind | null>(null);
  const [dragRow, setDragRow] = useState<EventKind | null>(null);
  const [volume, setVolume] = useState<number | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const active = config?.active_profile;
  const cur = useMemo(
    () => profiles?.find((p) => p.id === selected) ?? profiles?.find((p) => p.id === active) ?? profiles?.[0],
    [profiles, selected, active],
  );
  useEffect(() => setVolume(null), [cur?.id]);
  useEffect(() => {
    if (!addOpen) return;
    const close = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest("[data-add-menu]")) setAddOpen(null);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [addOpen]);

  if (!profiles || !cur) return null;
  const readOnly = cur.builtin;
  const builtin = profiles.filter((p) => p.builtin);
  const custom = profiles.filter((p) => !p.builtin);

  /** Optimistic edit of the current (user) profile. */
  const update = async (fn: (p: Profile) => void) => {
    if (readOnly) return;
    const next = structuredClone(cur);
    fn(next);
    setProfiles(profiles.map((p) => (p.id === next.id ? next : p)));
    try {
      await backend.saveProfile(next);
    } catch (e) {
      console.error(e);
      await reload();
    }
  };

  const addSound = (k: EventKind, id: string) => {
    setAddOpen(null);
    setDragRow(null);
    void update((p) => {
      const b = (p.mapping[k] ??= { enabled: true, sounds: [] });
      if (!b.sounds.includes(id)) b.sounds.push(id);
    });
  };

  const startRename = (p: Profile) => {
    setRenaming(p.id);
    setRenameVal(p.name);
  };
  const commitRename = async () => {
    const id = renaming;
    setRenaming(null);
    const v = renameVal.trim();
    if (!id || !v) return;
    await backend.renameProfile(id, v).catch(console.error);
    await reload();
  };

  const create = async () => {
    const p = await backend.createProfile(t("profiles.untitled"));
    await reload();
    setSelected(p.id);
    startRename(p);
  };
  const duplicate = async () => {
    const p = await backend.duplicateProfile(cur.id, t("profiles.copyOf", { name: profileName(t, cur) }));
    await reload();
    setSelected(p.id);
    startRename(p);
  };
  const remove = async () => {
    setConfirmDelete(false);
    await backend.deleteProfile(cur.id).catch(console.error);
    setSelected(null);
    await reload();
  };

  const listItem = (p: Profile) => {
    const sel = p.id === cur.id;
    if (renaming === p.id) {
      return (
        <input
          key={p.id}
          className={s.renameInput}
          aria-label={t("profiles.rename")}
          value={renameVal}
          autoFocus
          onChange={(e) => setRenameVal(e.target.value)}
          onBlur={() => void commitRename()}
          onKeyDown={(e) => {
            if (e.key === "Enter") void commitRename();
            if (e.key === "Escape") setRenaming(null);
          }}
        />
      );
    }
    return (
      <button
        key={p.id}
        type="button"
        role="option"
        aria-selected={sel}
        className={s.item}
        title={p.builtin ? undefined : t("profiles.renameHint")}
        onClick={() => {
          setSelected(p.id);
          setAddOpen(null);
        }}
        onDoubleClick={() => !p.builtin && startRename(p)}
      >
        <span className={s.itemText}>
          <span className={s.itemName}>{profileName(t, p)}</span>
          {p.builtin && <span className={s.itemDesc}>{profileDesc(t, p)}</span>}
        </span>
        {p.id === active && <span className={s.activeMark}>{t("common.active")}</span>}
        {p.builtin && (
          <span className={s.lock} title={t("common.readOnly")} aria-label={t("common.readOnly")}>
            <Icon name="lock" size={13} />
          </span>
        )}
      </button>
    );
  };

  const vol = volume ?? Math.round(cur.volume * 100);
  const curName = profileName(t, cur);

  return (
    <div className={s.page}>
      <div className={s.listCol}>
        <div role="listbox" aria-label={t("profiles.list")} className={s.list}>
          <div className={s.groupLabel}>{t("profiles.builtin")}</div>
          {builtin.map(listItem)}
          <div className={s.groupLabel} style={{ paddingTop: 14 }}>
            {t("profiles.custom")}
          </div>
          {custom.map(listItem)}
          {custom.length === 0 && <p className={s.noCustom}>{t("profiles.noCustom")}</p>}
        </div>
        <div role="toolbar" aria-label={t("profiles.toolbar")} className={s.toolbar}>
          <IconButton icon="plus" label={t("profiles.add")} onClick={() => void create()} />
          <IconButton
            icon="minus"
            label={t("profiles.delete")}
            disabled={readOnly}
            onClick={() => setConfirmDelete(true)}
          />
          <IconButton icon="copy" size={14} label={t("profiles.duplicate")} onClick={() => void duplicate()} />
        </div>
      </div>

      <div className={s.detail}>
        <div className={s.detailHead}>
          <h2 className={s.detailTitle}>{curName}</h2>
          {readOnly && <Badge tone="neutral">{t("common.readOnly")}</Badge>}
          {cur.id !== active && (
            <Button onClick={() => void backend.setActiveProfile(cur.id).then(reload)}>{t("profiles.useThis")}</Button>
          )}
          <div className={s.volume} style={{ opacity: readOnly ? 0.55 : 1 }}>
            <Icon name="vol" size={15} style={{ color: "var(--text-faint)" }} />
            <Slider
              label={t("profiles.volume")}
              value={vol}
              disabled={readOnly}
              onChange={setVolume}
              onCommit={(v) => void update((p) => (p.volume = v / 100))}
            />
          </div>
        </div>

        {readOnly && (
          <div role="note" className={s.note}>
            <Icon name="lock" size={13} style={{ color: "var(--color-accent)" }} />
            <span className={s.noteText}>{t("profiles.readOnlyNote")}</span>
            <Button variant="primary" onClick={() => void duplicate()}>
              {t("profiles.duplicateToEdit")}
            </Button>
          </div>
        )}

        <div role="list" aria-label={t("profiles.mapping")} className={s.rows}>
          {EVENT_KINDS.map((k) => {
            const b = cur.mapping[k] ?? { enabled: false, sounds: [] };
            const label = t(`events.${k}`);
            const options = (sounds ?? []).filter((x) => !b.sounds.includes(x.id));
            const onDragOver = (e: DragEvent) => {
              if (readOnly || !e.dataTransfer.types.includes(DRAG_TYPE)) return;
              e.preventDefault();
              e.dataTransfer.dropEffect = "copy";
              if (dragRow !== k) setDragRow(k);
            };
            const onDrop = (e: DragEvent) => {
              e.preventDefault();
              const id = e.dataTransfer.getData(DRAG_TYPE);
              if (id && !readOnly) addSound(k, id);
              else setDragRow(null);
            };
            return (
              <div
                key={k}
                role="listitem"
                className={s.row}
                data-drop={dragRow === k ? "" : undefined}
                onDragOver={onDragOver}
                onDragLeave={() => setDragRow(null)}
                onDrop={onDrop}
              >
                <div className={s.event}>
                  <span className={s.eventIcon}>
                    <Icon name={EVENT_ICONS[k]} />
                  </span>
                  <span style={{ minWidth: 0 }}>
                    <span className={s.eventLabel}>{label}</span>
                    <span className={s.eventHint}>{hint(t, k, config)}</span>
                  </span>
                </div>
                <div className={s.chips} style={{ opacity: b.enabled ? 1 : 0.45 }}>
                  {b.sounds.map((id) => {
                    const snd = index.get(id);
                    const name = soundName(t, snd, id);
                    const key = `${k}:${id}`;
                    const playing = preview.playing === key;
                    return (
                      <span
                        key={id}
                        draggable
                        onDragStart={(e) => {
                          e.dataTransfer.setData(DRAG_TYPE, id);
                          e.dataTransfer.effectAllowed = "copy";
                        }}
                        style={{ display: "flex" }}
                      >
                        <Tag
                          playing={playing}
                          onPlay={() => preview.toggle(key, snd)}
                          playLabel={t(playing ? "common.stop" : "common.play", { name })}
                          onRemove={
                            readOnly
                              ? undefined
                              : () => void update((p) => (p.mapping[k]!.sounds = b.sounds.filter((x) => x !== id)))
                          }
                          removeLabel={t("common.remove", { name })}
                        >
                          {name}
                        </Tag>
                      </span>
                    );
                  })}
                  {b.sounds.length === 0 && (
                    <span className={s.empty}>{t(readOnly ? "profiles.noSoundsReadOnly" : "profiles.noSounds")}</span>
                  )}
                  {!readOnly && (
                    <div style={{ position: "relative" }} data-add-menu="">
                      <button
                        type="button"
                        className={s.addBtn}
                        aria-expanded={addOpen === k}
                        aria-label={t("profiles.addSoundFor", { event: label })}
                        onClick={() => setAddOpen(addOpen === k ? null : k)}
                      >
                        <Icon name="plus" size={12} strokeWidth={2} />
                        {t("profiles.addSound")}
                      </button>
                      {addOpen === k && (
                        <div role="menu" className={s.menu}>
                          <div className={s.menuHint}>
                            {options.length ? t("profiles.addHint") : t("profiles.nothingToAdd")}
                          </div>
                          {options.map((o) => (
                            <button
                              key={o.id}
                              type="button"
                              role="menuitem"
                              draggable
                              className={s.menuItem}
                              onDragStart={(e) => {
                                e.dataTransfer.setData(DRAG_TYPE, o.id);
                                e.dataTransfer.effectAllowed = "copy";
                              }}
                              onClick={() => addSound(k, o.id)}
                            >
                              <Icon name="grip" size={12} style={{ color: "var(--text-faint)" }} />
                              <span style={{ flex: 1 }}>{soundName(t, o)}</span>
                              <span className={s.dur}>{formatDuration(o.duration_ms)}</span>
                            </button>
                          ))}
                        </div>
                      )}
                    </div>
                  )}
                </div>
                <Switch
                  checked={b.enabled}
                  disabled={readOnly}
                  label={t(b.enabled ? "profiles.switchOn" : "profiles.switchOff", { event: label })}
                  onChange={(v) =>
                    void update((p) => {
                      p.mapping[k] = { ...b, enabled: v };
                    })
                  }
                />
              </div>
            );
          })}
        </div>
        <p className={s.footnote}>{t("profiles.randomNote")}</p>
      </div>

      {confirmDelete && (
        <Sheet
          title={t("profiles.deleteTitle", { name: curName })}
          body={t(cur.id === active ? "profiles.deleteActiveBody" : "profiles.deleteBody")}
          confirmLabel={t("common.delete")}
          cancelLabel={t("common.cancel")}
          onCancel={() => setConfirmDelete(false)}
          onConfirm={() => void remove()}
        />
      )}
    </div>
  );
}
